use crate::{
    Error, Result,
    wire::{Reader, put_u16, put_u32},
};
use std::collections::HashSet;

pub const MAX_CHANNELS: usize = 64;
pub(crate) const MAX_CHANNEL_TEXT: usize = 96;
const U64_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;

crate::wire_enum! {
    /// 一列数据的存储值域、量化规则及压缩方式。
    pub enum ColumnEncoding { Percent = 1, Temperature = 2, Power = 3, Unsigned = 4, Rate = 5 }
}

impl ColumnEncoding {
    pub const fn max_value(self) -> u64 {
        match self {
            Self::Percent => 10_000,
            Self::Temperature | Self::Power => u16::MAX as u64,
            Self::Unsigned | Self::Rate => u64::MAX,
        }
    }

    pub const fn is_u16(self) -> bool {
        matches!(self, Self::Percent | Self::Temperature | Self::Power)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub key: String,
    pub unit: String,
    pub encoding: ColumnEncoding,
}

impl Channel {
    /// 量化原始浮点值；非有限值或超出值域时返回 `None`。
    /// 整数来源应直接传入精确的 `u64`，无需转换为浮点值。
    pub fn quantize(&self, source: f64) -> Option<u64> {
        let scaled = match self.encoding {
            ColumnEncoding::Percent => source * 100.0,
            ColumnEncoding::Temperature => (source + 100.0) * 100.0,
            ColumnEncoding::Power => source * 10.0,
            ColumnEncoding::Unsigned | ColumnEncoding::Rate => source,
        };
        if !(0.0..U64_EXCLUSIVE).contains(&scaled) {
            return None;
        }
        if self.encoding.is_u16() && scaled > self.encoding.max_value() as f64 {
            return None;
        }
        Some(scaled.round() as u64)
    }

    /// 将值域内的存储值转换为原单位。
    pub fn dequantize(&self, value: u64) -> Option<f64> {
        if value > self.encoding.max_value() {
            return None;
        }
        let value = value as f64;
        Some(match self.encoding {
            ColumnEncoding::Percent => value / 100.0,
            ColumnEncoding::Temperature => value / 100.0 - 100.0,
            ColumnEncoding::Power => value / 10.0,
            ColumnEncoding::Unsigned | ColumnEncoding::Rate => value,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub version: u32,
    pub channels: Vec<Channel>,
}

impl Catalog {
    /// 按第 1 版写入顺序返回标准列。读取方必须解析实际保存的目录，
    /// 按通道键查找列，不能假定文件中的顺序。
    pub fn standard(version: u32) -> Self {
        use ColumnEncoding::*;
        Self {
            version,
            channels: [
                ("cpu.usage", "%", Percent),
                ("gpu.usage", "%", Percent),
                ("cpu.temp", "°C", Temperature),
                ("gpu.temp", "°C", Temperature),
                ("cpu.power", "W", Power),
                ("gpu.power", "W", Power),
                ("mem.used", "B", Unsigned),
                ("mem.total", "B", Unsigned),
                ("gpu.mem.used", "B", Unsigned),
                ("gpu.mem.total", "B", Unsigned),
                ("net.rx.rate", "B/s", Rate),
                ("net.tx.rate", "B/s", Rate),
                ("uptime", "s", Unsigned),
            ]
            .into_iter()
            .map(|(key, unit, encoding)| Channel {
                key: key.into(),
                unit: unit.into(),
                encoding,
            })
            .collect(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.channels.len() > MAX_CHANNELS {
            return Err(Error::Limit("catalog channel count"));
        }
        if self.channels.is_empty() {
            return Err(Error::Invalid("empty catalog"));
        }
        let mut seen = HashSet::with_capacity(self.channels.len());
        for channel in &self.channels {
            if channel.key.len() > MAX_CHANNEL_TEXT || channel.unit.len() > MAX_CHANNEL_TEXT {
                return Err(Error::Limit("catalog text bytes"));
            }
            if channel.key.is_empty()
                || !channel
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(Error::Invalid("channel key"));
            }
            if channel.unit.chars().any(char::is_control) {
                return Err(Error::Invalid("channel unit"));
            }
            if !seen.insert(channel.key.as_str()) {
                return Err(Error::Invalid("duplicate channel key"));
            }
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = Vec::new();
        put_u32(&mut out, self.version);
        put_u16(&mut out, self.channels.len() as u16);
        for c in &self.channels {
            out.push(c.encoding as u8);
            for text in [&c.key, &c.unit] {
                out.push(text.len() as u8);
                out.extend_from_slice(text.as_bytes());
            }
        }
        Ok(out)
    }

    pub(crate) fn decode(payload: &[u8]) -> Result<Self> {
        fn string(r: &mut Reader<'_>) -> Result<String> {
            let len = usize::from(r.u8()?);
            if len > MAX_CHANNEL_TEXT {
                return Err(Error::Limit("catalog text bytes"));
            }
            let text =
                std::str::from_utf8(r.take(len)?).map_err(|_| Error::Invalid("catalog UTF-8"))?;
            Ok(text.to_owned())
        }
        let mut r = Reader::new(payload);
        let version = r.u32()?;
        let count = usize::from(r.u16()?);
        if count > MAX_CHANNELS {
            return Err(Error::Limit("catalog channel count"));
        }
        let mut channels = Vec::with_capacity(count);
        for _ in 0..count {
            channels.push(Channel {
                encoding: ColumnEncoding::from_byte(r.u8()?)?,
                key: string(&mut r)?,
                unit: string(&mut r)?,
            });
        }
        r.finish()?;
        let catalog = Self { version, channels };
        catalog.validate()?;
        Ok(catalog)
    }
}
