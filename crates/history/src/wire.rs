use crate::{Block, BlockHeader, Catalog, Error, Frame, Gap, Id, Marker, Packet, Result};

const DAY_MAGIC: [u8; 4] = *b"RHH\0";
const PUB_MAGIC: [u8; 4] = *b"RHP\0";
pub const FORMAT_VERSION: u16 = 1;
pub const DAY_HEADER_LEN: usize = 30;
pub const PUBLISH_BOUNDARY_LEN: usize = 34;
pub const PACKET_HEADER_LEN: usize = 5;
pub const PACKET_OVERHEAD: usize = PACKET_HEADER_LEN + 4;
pub const MAX_PACKET_LEN: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayHeader {
    /// 以 1970-01-01 为基准的带符号 UTC 日数。
    pub utc_day: i32,
    pub day_id: Id,
}

impl DayHeader {
    pub fn encode(&self) -> [u8; DAY_HEADER_LEN] {
        let mut out = [0; DAY_HEADER_LEN];
        out[..4].copy_from_slice(&DAY_MAGIC);
        out[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        out[6..10].copy_from_slice(&self.utc_day.to_le_bytes());
        out[10..26].copy_from_slice(&self.day_id);
        seal(&mut out);
        out
    }

    /// 读取固定长度的文件头，允许其后有数据包。
    pub fn decode(input: &[u8]) -> Result<Self> {
        let input = checked_header(input, DAY_MAGIC, DAY_HEADER_LEN)?;
        Ok(Self {
            utc_day: i32::from_le_bytes(input[6..10].try_into().unwrap()),
            day_id: input[10..26].try_into().unwrap(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishBoundary {
    pub day_id: Id,
    pub offset: u64,
}

impl PublishBoundary {
    pub fn encode(&self) -> [u8; PUBLISH_BOUNDARY_LEN] {
        let mut out = [0; PUBLISH_BOUNDARY_LEN];
        out[..4].copy_from_slice(&PUB_MAGIC);
        out[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        out[6..22].copy_from_slice(&self.day_id);
        out[22..30].copy_from_slice(&self.offset.to_le_bytes());
        seal(&mut out);
        out
    }

    /// 调用方还必须核对日代号及文件大小，并确认偏移位于包边界。
    /// 边界文件不允许有尾随字节。
    pub fn decode(input: &[u8]) -> Result<Self> {
        let prefix = checked_header(input, PUB_MAGIC, PUBLISH_BOUNDARY_LEN)?;
        if prefix.len() != input.len() {
            return Err(Error::Invalid("trailing publication bytes"));
        }
        let offset = u64::from_le_bytes(prefix[22..30].try_into().unwrap());
        if offset < DAY_HEADER_LEN as u64 {
            return Err(Error::Invalid("publication precedes day header"));
        }
        Ok(Self {
            day_id: prefix[6..22].try_into().unwrap(),
            offset,
        })
    }
}

fn seal(out: &mut [u8]) {
    let (protected, checksum) = out.split_at_mut(out.len() - 4);
    checksum.copy_from_slice(&crc32c::crc32c(protected).to_le_bytes());
}

fn verify_crc(input: &[u8]) -> Result<()> {
    let (protected, checksum) = input.split_at(input.len() - 4);
    if crc32c::crc32c(protected).to_le_bytes() != checksum {
        return Err(Error::Checksum);
    }
    Ok(())
}

fn checked_header(input: &[u8], magic: [u8; 4], len: usize) -> Result<&[u8]> {
    let leading = input.get(..6).ok_or(Error::Truncated)?;
    if leading[..4] != magic {
        return Err(Error::Invalid("file magic"));
    }
    let version = u16::from_le_bytes(leading[4..6].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let prefix = input.get(..len).ok_or(Error::Truncated)?;
    verify_crc(prefix)?;
    Ok(prefix)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketTag {
    Catalog = 1,
    Frame = 2,
    Marker = 3,
    Block = 4,
    Gap = 5,
    Roll = 6,
}

impl PacketTag {
    fn from_byte(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Catalog),
            2 => Ok(Self::Frame),
            3 => Ok(Self::Marker),
            4 => Ok(Self::Block),
            5 => Ok(Self::Gap),
            6 => Ok(Self::Roll),
            _ => Err(Error::UnknownTag(value)),
        }
    }

    fn validate_len(self, len: usize) -> Result<()> {
        let plausible = match self {
            Self::Catalog => (6..=6 + crate::MAX_CHANNELS
                * (3 + 2 * crate::catalog::MAX_CHANNEL_TEXT))
                .contains(&len),
            Self::Frame => {
                (21..=45 + crate::MAX_CHANNELS.div_ceil(8) + 8 * crate::MAX_CHANNELS).contains(&len)
            }
            Self::Marker => matches!(len, 18 | 34) || (35..=67).contains(&len),
            Self::Block => len >= crate::block::MIN_BLOCK_PAYLOAD,
            Self::Gap => len == 41,
            Self::Roll => true,
        };
        if !plausible {
            return Err(Error::Invalid("packet payload length for tag"));
        }
        Ok(())
    }
}

/// 经 CRC 校验的包；解析为具体类型时仍会验证所有载荷字段。
/// FRAME 的目录取自 WAL 上下文；BLOCK 保存目录版本。
#[derive(Clone, Copy, Debug)]
pub struct RawPacket<'a> {
    pub tag: PacketTag,
    pub payload: &'a [u8],
}

impl RawPacket<'_> {
    pub fn decode(&self, catalog: Option<&Catalog>) -> Result<Packet> {
        if self.payload.len() > MAX_PACKET_LEN - PACKET_OVERHEAD {
            return Err(Error::Limit("packet bytes"));
        }
        self.tag.validate_len(self.payload.len())?;
        Ok(match self.tag {
            PacketTag::Catalog => Packet::Catalog(Catalog::decode(self.payload)?),
            PacketTag::Frame => {
                Packet::Frame(Frame::decode(self.payload, catalog.ok_or(Error::Catalog)?)?)
            }
            PacketTag::Marker => Packet::Marker(Marker::decode(self.payload)?),
            PacketTag::Block => {
                Packet::Block(Block::decode(self.payload, catalog.ok_or(Error::Catalog)?)?)
            }
            PacketTag::Gap => Packet::Gap(Gap::decode(self.payload)?),
            PacketTag::Roll => Packet::Roll(self.payload.to_vec()),
        })
    }

    pub fn catalog_version(&self) -> Result<Option<u32>> {
        if self.tag == PacketTag::Block {
            Ok(Some(Reader::new(self.payload).u32()?))
        } else {
            Ok(None)
        }
    }

    /// 读取索引所需的 BLOCK 元数据而不解压列值。
    /// 此操作不校验压缩帧，也不解析它引用的定义。
    pub fn block_header(&self) -> Result<Option<BlockHeader>> {
        if self.tag == PacketTag::Block {
            Ok(Some(BlockHeader::decode(&mut Reader::new(self.payload))?))
        } else {
            Ok(None)
        }
    }
}

/// 在分配内存或读取载荷前，先验证包头的 5 个字节。
pub fn packet_len(header: &[u8]) -> Result<usize> {
    let header = header.get(..PACKET_HEADER_LEN).ok_or(Error::Truncated)?;
    let tag = PacketTag::from_byte(header[0])?;
    let payload_len = u32::from_le_bytes(header[1..].try_into().unwrap());
    if payload_len > (MAX_PACKET_LEN - PACKET_OVERHEAD) as u32 {
        return Err(Error::Limit("packet bytes"));
    }
    tag.validate_len(payload_len as usize)?;
    Ok(PACKET_OVERHEAD + payload_len as usize)
}

/// 切片必须已限制在可读文件范围内；只解析切片中的第一个包。
/// 返回消耗的字节数，而不是下一个包的文件偏移。
pub fn decode_packet(input: &[u8]) -> Result<(RawPacket<'_>, usize)> {
    let len = packet_len(input)?;
    let packet = input.get(..len).ok_or(Error::Truncated)?;
    verify_crc(packet)?;
    Ok((
        RawPacket {
            tag: PacketTag::from_byte(packet[0])?,
            payload: &packet[PACKET_HEADER_LEN..len - 4],
        },
        len,
    ))
}

pub(crate) fn encode_packet(tag: PacketTag, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PACKET_LEN - PACKET_OVERHEAD {
        return Err(Error::Limit("packet bytes"));
    }
    tag.validate_len(payload.len())?;
    let mut out = Vec::with_capacity(PACKET_OVERHEAD + payload.len());
    out.push(tag as u8);
    put_u32(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    let checksum = crc32c::crc32c(&out);
    put_u32(&mut out, checksum);
    Ok(out)
}

pub(crate) fn validate_session_ref(offset: u64) -> Result<()> {
    if offset < DAY_HEADER_LEN as u64 {
        return Err(Error::Session);
    }
    Ok(())
}

pub(crate) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(crate) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(crate) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(crate) fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_var(out: &mut Vec<u8>, mut value: u128) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let (taken, left) = self
            .bytes
            .split_at_checked(n)
            .ok_or(Error::Invalid("short payload"))?;
        self.bytes = left;
        Ok(taken)
    }
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.bytes
    }
    pub(crate) fn finish(self) -> Result<()> {
        if !self.bytes.is_empty() {
            return Err(Error::Invalid("trailing payload bytes"));
        }
        Ok(())
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub(crate) fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub(crate) fn id(&mut self) -> Result<Id> {
        Ok(self.take(16)?.try_into().unwrap())
    }
    pub(crate) fn present(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Invalid("invalid presence flag")),
        }
    }
    pub(crate) fn bitmap(&mut self, count: usize) -> Result<&'a [u8]> {
        let bytes = self.take(count.div_ceil(8))?;
        let used = count % 8;
        if used != 0 && bytes[bytes.len() - 1] >> used != 0 {
            return Err(Error::Invalid("nonzero bitmap padding"));
        }
        Ok(bytes)
    }
    // 第 1 版中最宽的值是两个 u64 的差值经 zigzag 编码后的结果。
    pub(crate) fn var(&mut self, max: u128) -> Result<u128> {
        let mut value = 0u128;
        for shift in (0..70).step_by(7) {
            let byte = self.u8()?;
            value |= u128::from(byte & 127) << shift;
            if byte & 128 == 0 {
                if (shift != 0 && byte == 0) || value > max {
                    return Err(Error::Invalid("noncanonical or overflowing varint"));
                }
                return Ok(value);
            }
        }
        Err(Error::Invalid("overlong varint"))
    }
}

pub(crate) struct Bitmap(Vec<u8>);
impl Bitmap {
    pub(crate) fn zeroed(count: usize) -> Self {
        Self(vec![0; count.div_ceil(8)])
    }
    pub(crate) fn set(&mut self, index: usize) {
        self.0[index / 8] |= 1 << (index % 8);
    }
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}
pub(crate) fn bit(bytes: &[u8], index: usize) -> bool {
    bytes[index / 8] & (1 << (index % 8)) != 0
}
