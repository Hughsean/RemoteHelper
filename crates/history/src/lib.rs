//! RemoteHelper 历史记录格式第 1 版，详见[字节布局](../FORMAT.md)。
//!
//! 存储层提供限制在已提交前缀内的切片，并判断断尾或损坏是否可恢复。
//! 本库不会修改文件。

mod block;
mod catalog;
mod context;
mod wire;

pub use block::{
    Block, BlockHeader, DEFAULT_COMPRESSION_LEVEL, MAX_BLOCK_DECOMPRESSED, MAX_BLOCK_POINTS,
};
pub use catalog::{Catalog, Channel, ColumnEncoding, MAX_CHANNELS};
pub use context::{FileContext, FileKind};
pub use wire::{
    DAY_HEADER_LEN, DayHeader, FORMAT_VERSION, MAX_PACKET_LEN, PACKET_HEADER_LEN, PACKET_OVERHEAD,
    PUBLISH_BOUNDARY_LEN, PacketTag, PublishBoundary, RawPacket, decode_packet, packet_len,
};

use wire::{Reader, put_i64, put_u32, put_u64};

/// 标识符按 UUID 字符串对应的字节顺序保存，而不是 Windows GUID 的内存顺序。
pub type Id = [u8; 16];
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("incomplete history header or packet")]
    Truncated,
    #[error("unsupported history format version {0}; upgrade to read this file")]
    UnsupportedVersion(u16),
    #[error("unknown packet tag {0:#04x}")]
    UnknownTag(u8),
    #[error("history CRC32C mismatch")]
    Checksum,
    #[error("invalid history data: {0}")]
    Invalid(&'static str),
    #[error("history limit exceeded: {0}")]
    Limit(&'static str),
    #[error("catalog definition missing or mismatched")]
    Catalog,
    #[error("session definition missing or invalid")]
    Session,
    #[error("invalid or unsupported zstd frame")]
    ZstdFrame,
    #[error("history zstd codec: {0}")]
    Compression(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Catalog(Catalog),
    Frame(Frame),
    Marker(Marker),
    Block(Block),
    Gap(Gap),
    /// 预留的小时汇总载荷；第 1 版不解析内容。
    Roll(Vec<u8>),
}

impl Packet {
    /// FRAME 和 BLOCK 需要对应的目录。用 `Block::encode_packet` 指定压缩级别，
    /// 或用 `Block::encode_split` 拆分超出单块上限的批次。
    pub fn encode(&self, catalog: Option<&Catalog>) -> Result<Vec<u8>> {
        use PacketTag as Tag;
        let (tag, payload) = match self {
            Self::Catalog(definition) => (Tag::Catalog, definition.encode()?),
            Self::Frame(frame) => (Tag::Frame, frame.encode(catalog.ok_or(Error::Catalog)?)?),
            Self::Marker(marker) => (Tag::Marker, marker.encode()?),
            Self::Block(block) => {
                return block
                    .encode_packet(catalog.ok_or(Error::Catalog)?, DEFAULT_COMPRESSION_LEVEL);
            }
            Self::Gap(gap) => (Tag::Gap, gap.encode()?),
            Self::Roll(payload) => return wire::encode_packet(Tag::Roll, payload),
        };
        wire::encode_packet(tag, &payload)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockEvidence {
    pub wall_ms: Option<i64>,
    pub tick_ms: Option<u64>,
    pub awake_ms: Option<u64>,
}

impl ClockEvidence {
    fn encode(self, out: &mut Vec<u8>) {
        out.push(
            u8::from(self.wall_ms.is_some())
                | u8::from(self.tick_ms.is_some()) << 1
                | u8::from(self.awake_ms.is_some()) << 2,
        );
        if let Some(wall) = self.wall_ms {
            put_i64(out, wall);
        }
        if let Some(tick) = self.tick_ms {
            put_u64(out, tick);
        }
        if let Some(awake) = self.awake_ms {
            put_u64(out, awake);
        }
    }

    fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let flags = r.u8()?;
        if flags & !7 != 0 {
            return Err(Error::Invalid("reserved clock flags"));
        }
        Ok(Self {
            wall_ms: (flags & 1 != 0).then(|| r.i64()).transpose()?,
            tick_ms: (flags & 2 != 0).then(|| r.u64()).transpose()?,
            awake_ms: (flags & 4 != 0).then(|| r.u64()).transpose()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub sample_seq: u64,
    pub time_ms: i64,
    pub period_ms: u32,
    pub clock: ClockEvidence,
    /// 按目录顺序排列；整数已经量化，`None` 表示缺失。
    pub values: Vec<Option<u64>>,
}

impl Frame {
    fn validate(&self, catalog: &Catalog) -> Result<()> {
        if self.period_ms == 0 {
            return Err(Error::Invalid("zero sample period"));
        }
        if self.values.len() != catalog.channels.len() {
            return Err(Error::Invalid("frame column count"));
        }
        for (channel, value) in catalog.channels.iter().zip(&self.values) {
            if value.is_some_and(|value| value > channel.encoding.max_value()) {
                return Err(Error::Invalid("column value outside domain"));
            }
        }
        Ok(())
    }

    fn encode(&self, catalog: &Catalog) -> Result<Vec<u8>> {
        catalog.validate()?;
        self.validate(catalog)?;
        let mut out = Vec::new();
        put_u64(&mut out, self.sample_seq);
        put_i64(&mut out, self.time_ms);
        put_u32(&mut out, self.period_ms);
        self.clock.encode(&mut out);
        let mut bits = wire::Bitmap::zeroed(self.values.len());
        for (i, value) in self.values.iter().enumerate() {
            if value.is_some() {
                bits.set(i);
            }
        }
        out.extend_from_slice(bits.as_slice());
        for (channel, value) in catalog.channels.iter().zip(&self.values) {
            if let Some(value) = value {
                if channel.encoding.is_u16() {
                    wire::put_u16(&mut out, *value as u16);
                } else {
                    put_u64(&mut out, *value);
                }
            }
        }
        Ok(out)
    }

    fn decode(payload: &[u8], catalog: &Catalog) -> Result<Self> {
        catalog.validate()?;
        let mut r = Reader::new(payload);
        let sample_seq = r.u64()?;
        let time_ms = r.i64()?;
        let period_ms = r.u32()?;
        let clock = ClockEvidence::decode(&mut r)?;
        let bits = r.bitmap(catalog.channels.len())?;
        let mut values = Vec::with_capacity(catalog.channels.len());
        for (i, channel) in catalog.channels.iter().enumerate() {
            values.push(if wire::bit(bits, i) {
                Some(if channel.encoding.is_u16() {
                    u64::from(r.u16()?)
                } else {
                    r.u64()?
                })
            } else {
                None
            });
        }
        r.finish()?;
        let frame = Self {
            sample_seq,
            time_ms,
            period_ms,
            clock,
            values,
        };
        frame.validate(catalog)?;
        Ok(frame)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub session_id: Id,
    pub boot_id: Option<Id>,
}

impl Session {
    fn validate(&self) -> Result<()> {
        if self.boot_id == Some([0; 16]) {
            return Err(Error::Invalid("nil boot ID must be absent"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Session(Session),
    Event(Event),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: MarkerKind,
    pub event_id: Id,
    pub segment: u64,
    pub session_ref: u64,
    pub sample_seq: Option<u64>,
    pub clock: ClockEvidence,
}

wire_enum! {
    /// 带文件内会话引用的生命周期及分段事件。
    pub enum MarkerKind { Start = 1, Exit = 2, Disabled = 3, Resumed = 4, ClockSegment = 5, Continuation = 6 }
}

impl Marker {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::Session(s) => {
                s.validate()?;
                out.push(0);
                out.extend_from_slice(&s.session_id);
                out.push(u8::from(s.boot_id.is_some()));
                if let Some(boot) = s.boot_id {
                    out.extend_from_slice(&boot);
                }
            }
            Self::Event(e) => {
                wire::validate_session_ref(e.session_ref)?;
                out.push(e.kind as u8);
                out.extend_from_slice(&e.event_id);
                put_u64(&mut out, e.segment);
                put_u64(&mut out, e.session_ref);
                out.push(u8::from(e.sample_seq.is_some()));
                if let Some(seq) = e.sample_seq {
                    put_u64(&mut out, seq);
                }
                e.clock.encode(&mut out);
            }
        }
        Ok(out)
    }

    fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = Reader::new(payload);
        let kind = r.u8()?;
        let result = if kind == 0 {
            let session = Session {
                session_id: r.id()?,
                boot_id: r.present()?.then(|| r.id()).transpose()?,
            };
            session.validate()?;
            Self::Session(session)
        } else {
            let event = Event {
                kind: MarkerKind::from_byte(kind)?,
                event_id: r.id()?,
                segment: r.u64()?,
                session_ref: r.u64()?,
                sample_seq: r.present()?.then(|| r.u64()).transpose()?,
                clock: ClockEvidence::decode(&mut r)?,
            };
            wire::validate_session_ref(event.session_ref)?;
            Self::Event(event)
        };
        r.finish()?;
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub from_ms: i64,
    pub to_ms: i64,
    pub reason: GapReason,
    pub segment: u64,
    pub event_id: Id,
}

wire_enum! {
    /// 非空半开区间的缺口原因；时钟回拨使用点标记。
    pub enum GapReason {
        Disabled = 1, Storage = 2, RecorderOverrun = 3, Rebooted = 4,
        Stopped = 5, Interrupted = 6, Sleep = 7, ClockJump = 8, Stalled = 9, Unknown = 10
    }
}

impl Gap {
    fn validate(&self) -> Result<()> {
        if self.from_ms >= self.to_ms {
            return Err(Error::Invalid("empty or reversed gap"));
        }
        Ok(())
    }

    fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = Vec::with_capacity(41);
        put_i64(&mut out, self.from_ms);
        put_i64(&mut out, self.to_ms);
        out.push(self.reason as u8);
        put_u64(&mut out, self.segment);
        out.extend_from_slice(&self.event_id);
        Ok(out)
    }

    fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = Reader::new(payload);
        let gap = Self {
            from_ms: r.i64()?,
            to_ms: r.i64()?,
            reason: GapReason::from_byte(r.u8()?)?,
            segment: r.u64()?,
            event_id: r.id()?,
        };
        r.finish()?;
        gap.validate()?;
        Ok(gap)
    }
}

/// 枚举的字节判别值是持久化格式的一部分。
#[macro_export]
#[doc(hidden)]
macro_rules! wire_enum {
    ($(#[$attr:meta])* pub enum $name:ident { $($variant:ident = $value:literal),* $(,)? }) => {
        $(#[$attr])*
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant = $value),* }
        impl $name {
            fn from_byte(value: u8) -> $crate::Result<Self> {
                match value {
                    $($value => Ok(Self::$variant),)*
                    _ => Err($crate::Error::Invalid(concat!("unknown ", stringify!($name)))),
                }
            }
        }
    };
}
