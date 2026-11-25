use crate::{
    Catalog, ClockEvidence, Error, Frame, Result,
    wire::{self, Bitmap, PacketTag, Reader, put_i64, put_u16, put_u32, put_u64, put_var},
};
use zstd::zstd_safe::{self, CParameter, DParameter};

pub const MAX_BLOCK_POINTS: usize = 2_048;
pub const MAX_BLOCK_DECOMPRESSED: usize = 512 * 1024;
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;
const WINDOW_LOG: u32 = 19;
const U64_MAX: u128 = u64::MAX as u128;
const MAX_DELTA: u128 = 2 * U64_MAX;
pub(crate) const MIN_BLOCK_PAYLOAD: usize = 59 + 4; // 定长字段、时钟标志和 zstd 魔数

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub catalog_version: u32,
    pub segment: u64,
    /// 同一物理文件内先前 MARKER(Session) 的起始偏移。
    pub session_ref: u64,
    /// 磁盘上仅保留最后一个样本的时钟证据。
    /// 此前的样本解码后 `ClockEvidence` 为空，但仍有 `time_ms`。
    pub frames: Vec<Frame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHeader {
    pub catalog_version: u32,
    pub segment: u64,
    pub session_ref: u64,
    pub first_seq: u64,
    pub last_seq: u64,
    /// 最早采样时刻（含），而不是展示覆盖区间的起点。
    pub first_time_ms: i64,
    /// 最晚采样时刻（含），而不是展示覆盖区间的终点。
    pub last_time_ms: i64,
    pub count: u16,
    pub decompressed_len: u32,
    pub last_clock: ClockEvidence,
}

impl BlockHeader {
    fn from_frames(version: u32, segment: u64, session_ref: u64, frames: &[Frame]) -> Result<Self> {
        if frames.len() > MAX_BLOCK_POINTS {
            return Err(Error::Limit("block point count"));
        }
        let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
            return Err(Error::Invalid("empty block"));
        };
        wire::validate_session_ref(session_ref)?;
        Ok(Self {
            catalog_version: version,
            segment,
            session_ref,
            first_seq: first.sample_seq,
            last_seq: last.sample_seq,
            first_time_ms: first.time_ms,
            last_time_ms: last.time_ms,
            count: frames.len() as u16,
            decompressed_len: 0,
            last_clock: last.clock,
        })
    }

    fn encode(self, out: &mut Vec<u8>) {
        put_u32(out, self.catalog_version);
        put_u64(out, self.segment);
        put_u64(out, self.session_ref);
        put_u64(out, self.first_seq);
        put_u64(out, self.last_seq);
        put_i64(out, self.first_time_ms);
        put_i64(out, self.last_time_ms);
        put_u16(out, self.count);
        put_u32(out, self.decompressed_len);
        self.last_clock.encode(out);
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let header = Self {
            catalog_version: r.u32()?,
            segment: r.u64()?,
            session_ref: r.u64()?,
            first_seq: r.u64()?,
            last_seq: r.u64()?,
            first_time_ms: r.i64()?,
            last_time_ms: r.i64()?,
            count: r.u16()?,
            decompressed_len: r.u32()?,
            last_clock: ClockEvidence::decode(r)?,
        };
        wire::validate_session_ref(header.session_ref)?;
        let count = header.count as usize;
        let decompressed_len = header.decompressed_len as usize;
        if count > MAX_BLOCK_POINTS {
            return Err(Error::Limit("block point count"));
        }
        if decompressed_len > MAX_BLOCK_DECOMPRESSED {
            return Err(Error::Limit("block decompressed bytes"));
        }
        if count == 0
            || header.first_time_ms > header.last_time_ms
            || header
                .last_seq
                .checked_sub(header.first_seq)
                .is_none_or(|span| span < u64::from(header.count - 1))
        {
            return Err(Error::Invalid("block sample range"));
        }
        if count == 1
            && (header.first_seq != header.last_seq || header.first_time_ms != header.last_time_ms)
        {
            return Err(Error::Invalid("single-point block range"));
        }
        if decompressed_len < count * 3 {
            return Err(Error::Invalid("block decompressed length"));
        }
        Ok(header)
    }
}

impl Block {
    /// 编码一个有界的 BLOCK；超出限制需要拆块时返回 `Error::Limit`。
    pub fn encode_packet(&self, catalog: &Catalog, compression_level: i32) -> Result<Vec<u8>> {
        self.validate_catalog(catalog)?;
        self.encode_frames(catalog, compression_level, &self.frames)
    }

    /// 将一批样本编码成连续的有界块，保留样本顺序、段号和会话。
    /// 每块各自保留末样本的时钟证据。
    pub fn encode_split(&self, catalog: &Catalog, compression_level: i32) -> Result<Vec<Vec<u8>>> {
        self.validate_catalog(catalog)?;
        if self.frames.is_empty() {
            return Err(Error::Invalid("empty block"));
        }
        check_order(&self.frames)?;
        let mut packets = Vec::new();
        for frames in self.frames.chunks(MAX_BLOCK_POINTS) {
            let mut pending = vec![frames];
            while let Some(frames) = pending.pop() {
                match self.encode_frames(catalog, compression_level, frames) {
                    Ok(packet) => packets.push(packet),
                    Err(Error::Limit(_)) if frames.len() > 1 => {
                        let (first, second) = frames.split_at(frames.len() / 2);
                        pending.push(second);
                        pending.push(first);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(packets)
    }

    fn validate_catalog(&self, catalog: &Catalog) -> Result<()> {
        if self.catalog_version != catalog.version {
            return Err(Error::Catalog);
        }
        catalog.validate()
    }

    fn encode_frames(&self, catalog: &Catalog, level: i32, frames: &[Frame]) -> Result<Vec<u8>> {
        let mut header =
            BlockHeader::from_frames(self.catalog_version, self.segment, self.session_ref, frames)?;
        check_order(frames)?;
        let mut data = Vec::new();
        let mut prev_seq = header.first_seq;
        for frame in frames {
            frame.validate(catalog)?;
            put_var(
                &mut data,
                (i128::from(frame.time_ms) - i128::from(header.first_time_ms)) as u128,
            );
            put_var(&mut data, u128::from(frame.sample_seq - prev_seq));
            put_var(&mut data, u128::from(frame.period_ms));
            prev_seq = frame.sample_seq;
        }
        for (column, channel) in catalog.channels.iter().enumerate() {
            let mut bitmap = Bitmap::zeroed(frames.len());
            for (index, row) in frames.iter().enumerate() {
                if row.values[column].is_some() {
                    bitmap.set(index);
                }
            }
            data.extend_from_slice(bitmap.as_slice());
            let mut prev = None;
            for value in frames.iter().filter_map(|row| row.values[column]) {
                let coded = match prev {
                    Some(prev) if channel.encoding != crate::ColumnEncoding::Rate => {
                        zigzag(i128::from(value) - i128::from(prev))
                    }
                    _ => u128::from(value),
                };
                put_var(&mut data, coded);
                prev = Some(value);
            }
            if data.len() > MAX_BLOCK_DECOMPRESSED {
                return Err(Error::Limit("block decompressed bytes"));
            }
        }
        header.decompressed_len = data.len() as u32;
        let mut compressor = zstd::bulk::Compressor::new(level)?;
        compressor.set_parameter(CParameter::WindowLog(WINDOW_LOG))?;
        let compressed = compressor.compress(&data)?;
        let mut payload = Vec::new();
        header.encode(&mut payload);
        payload.extend_from_slice(&compressed);
        wire::encode_packet(PacketTag::Block, &payload)
    }

    pub(crate) fn decode(payload: &[u8], catalog: &Catalog) -> Result<Self> {
        catalog.validate()?;
        let mut r = Reader::new(payload);
        let header = BlockHeader::decode(&mut r)?;
        if header.catalog_version != catalog.version {
            return Err(Error::Catalog);
        }
        let count = header.count as usize;
        let expected = header.decompressed_len as usize;
        if expected < count * 3 + catalog.channels.len() * count.div_ceil(8) {
            return Err(Error::Invalid("block decompressed length"));
        }
        let compressed = r.rest();
        let data = decompress(compressed, expected)?;
        let mut r = Reader::new(&data);
        let mut frames = Vec::with_capacity(count);
        let mut prev_seq = header.first_seq;
        let mut prev_time = header.first_time_ms;
        for i in 0..count {
            let time_offset = r.var(U64_MAX)?;
            let seq_delta = r.var(U64_MAX)? as u64;
            let period = r.var(u128::from(u32::MAX))? as u32;
            if period == 0
                || (i == 0 && (time_offset != 0 || seq_delta != 0))
                || (i > 0 && seq_delta == 0)
            {
                return Err(Error::Invalid("block point identity or period"));
            }
            let time = i64::try_from(i128::from(header.first_time_ms) + time_offset as i128)
                .map_err(|_| Error::Invalid("sample time overflow"))?;
            let seq = prev_seq
                .checked_add(seq_delta)
                .ok_or(Error::Invalid("sample sequence overflow"))?;
            if time < prev_time || time > header.last_time_ms || seq > header.last_seq {
                return Err(Error::Invalid("block point outside range"));
            }
            let clock = if i + 1 == count {
                header.last_clock
            } else {
                ClockEvidence::default()
            };
            frames.push(Frame {
                time_ms: time,
                sample_seq: seq,
                period_ms: period,
                clock,
                values: Vec::with_capacity(catalog.channels.len()),
            });
            prev_seq = seq;
            prev_time = time;
        }
        if prev_seq != header.last_seq || prev_time != header.last_time_ms {
            return Err(Error::Invalid("block end does not match sample range"));
        }
        for channel in &catalog.channels {
            let bits = r.bitmap(count)?;
            let mut prev = None;
            let max = channel.encoding.max_value();
            for (i, frame) in frames.iter_mut().enumerate() {
                if !wire::bit(bits, i) {
                    frame.values.push(None);
                    continue;
                }
                let value = match prev {
                    Some(prev) if channel.encoding != crate::ColumnEncoding::Rate => {
                        let value = i128::from(prev) + unzigzag(r.var(MAX_DELTA)?);
                        let value = u64::try_from(value)
                            .map_err(|_| Error::Invalid("column delta overflow"))?;
                        if value > max {
                            return Err(Error::Invalid("column value outside domain"));
                        }
                        value
                    }
                    _ => r.var(u128::from(max))? as u64,
                };
                frame.values.push(Some(value));
                prev = Some(value);
            }
        }
        r.finish()?;
        Ok(Self {
            catalog_version: header.catalog_version,
            segment: header.segment,
            session_ref: header.session_ref,
            frames,
        })
    }
}

fn check_order(frames: &[Frame]) -> Result<()> {
    if frames
        .windows(2)
        .any(|pair| pair[0].sample_seq >= pair[1].sample_seq || pair[0].time_ms > pair[1].time_ms)
    {
        return Err(Error::Invalid("unordered points in a block"));
    }
    Ok(())
}

fn zigzag(delta: i128) -> u128 {
    ((delta << 1) ^ (delta >> 127)) as u128
}
fn unzigzag(delta: u128) -> i128 {
    ((delta >> 1) as i128) ^ -((delta & 1) as i128)
}

fn decompress(compressed: &[u8], expected: usize) -> Result<Vec<u8>> {
    if !compressed.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
        || zstd_safe::find_frame_compressed_size(compressed).ok() != Some(compressed.len())
    {
        return Err(Error::ZstdFrame);
    }
    let frame_descriptor = *compressed.get(4).ok_or(Error::ZstdFrame)?;
    if frame_descriptor & 0x20 == 0 {
        // 非单段帧的窗口描述符决定内存需求，不能由解压长度推断。
        let descriptor = *compressed.get(5).ok_or(Error::ZstdFrame)?;
        let base = 1u64 << (10 + (descriptor >> 3));
        let window = base + (base >> 3) * u64::from(descriptor & 7);
        if window > 1 << WINDOW_LOG {
            return Err(Error::Limit("zstd window bytes"));
        }
    }
    if let Some(actual) =
        zstd_safe::get_frame_content_size(compressed).map_err(|_| Error::ZstdFrame)?
        && actual != expected as u64
    {
        return Err(Error::Invalid("zstd frame content size"));
    }
    // zstd 仅在流式解压时强制执行窗口限制。
    // 输出缓冲区长度固定，帧未声明解压长度时也不会突破上限。
    use std::io::Read;
    let mut decoder = zstd::stream::read::Decoder::with_buffer(compressed)?;
    decoder.set_parameter(DParameter::WindowLogMax(WINDOW_LOG))?;
    let mut data = vec![0; expected];
    decoder.read_exact(&mut data)?;
    if decoder.read(&mut [0; 1])? != 0 {
        return Err(Error::Invalid("extra zstd output"));
    }
    Ok(data)
}
