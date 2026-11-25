use std::{collections::HashMap, sync::Arc};

use ::history::{Catalog, DayHeader, FileContext, FileKind, Marker, Packet, decode_packet};
use base64::{Engine, prelude::BASE64_STANDARD};
use common::{
    SampleIdentity,
    history_api::{Id, PACKET_PAGE_BUDGET, PacketPage, TimeRange},
};

use super::*;

pub(super) struct Decoder {
    pub header: DayHeader,
    pub offset: u64,
    pub data_range: Option<TimeRange>,
    pub gap_range: Option<TimeRange>,
    context: FileContext,
    catalogs: HashMap<u32, Arc<Catalog>>,
    segment: u64,
    session_definitions: usize,
}

impl Decoder {
    pub fn new(bytes: &[u8], kind: FileKind, expected: Option<DayHeader>) -> HistoryResult<Self> {
        let header = DayHeader::decode(bytes)?;
        if expected.is_some_and(|e| e != header) {
            return Err(HistoryError::Protocol("日文件头与目录不符"));
        }
        Ok(Self {
            header,
            offset: HEADER,
            data_range: None,
            gap_range: None,
            context: FileContext::new(kind),
            catalogs: HashMap::new(),
            segment: 0,
            session_definitions: 0,
        })
    }

    pub fn packet(
        &mut self,
        bytes: &[u8],
        mut playback: Option<(&mut Playback, HistorySource)>,
    ) -> HistoryResult<usize> {
        let (raw, len) = decode_packet(bytes)?;
        match self.context.decode_at(self.offset, &raw)? {
            Packet::Catalog(catalog) => {
                self.catalogs
                    .entry(catalog.version)
                    .or_insert_with(|| Arc::new(catalog));
            }
            Packet::Block(block) => {
                let session = self
                    .context
                    .session(block.session_ref)
                    .ok_or(::history::Error::Session)?
                    .session_id;
                let catalog = self
                    .catalogs
                    .get(&block.catalog_version)
                    .ok_or(::history::Error::Catalog)?;
                for frame in block.frames {
                    Self::sample(
                        self.header.utc_day,
                        &mut self.data_range,
                        frame,
                        session,
                        block.segment,
                        catalog,
                        &mut playback,
                    )?;
                }
            }
            Packet::Frame(frame) => {
                let session = self
                    .context
                    .current_session()
                    .ok_or(::history::Error::Session)?
                    .1
                    .session_id;
                let version = self
                    .context
                    .current_catalog()
                    .ok_or(::history::Error::Catalog)?
                    .version;
                let catalog = self
                    .catalogs
                    .get(&version)
                    .ok_or(::history::Error::Catalog)?;
                Self::sample(
                    self.header.utc_day,
                    &mut self.data_range,
                    frame,
                    session,
                    self.segment,
                    catalog,
                    &mut playback,
                )?;
            }
            Packet::Gap(gap) => {
                extend_range(&mut self.gap_range, gap.from_ms, gap.to_ms);
                if let Some((out, source)) = playback {
                    out.gap(gap, source);
                }
            }
            Packet::Marker(Marker::Event(event)) => {
                self.segment = event.segment;
                if let Some((out, source)) = playback {
                    out.event(
                        event,
                        self.context
                            .session(event.session_ref)
                            .ok_or(::history::Error::Session)?
                            .session_id,
                        source,
                    );
                }
            }
            Packet::Marker(Marker::Session(_)) => self.session_definitions += 1,
            Packet::Roll(_) => {}
        }
        if self.catalogs.len() > 4096 || self.session_definitions > 65_536 {
            return Err(HistoryError::Limit("日文件目录或会话定义"));
        }
        self.offset = self
            .offset
            .checked_add(len as u64)
            .ok_or(HistoryError::Limit("包偏移"))?;
        Ok(len)
    }

    fn sample(
        day: i32,
        range: &mut Option<TimeRange>,
        frame: Frame,
        session: Id,
        segment: u64,
        catalog: &Arc<Catalog>,
        playback: &mut Option<(&mut Playback, HistorySource)>,
    ) -> HistoryResult<()> {
        if day_of(frame.time_ms) != Some(day) {
            return Err(HistoryError::Protocol("样本不属于文件标记的 UTC 日"));
        }
        extend_range(range, frame.time_ms, frame.time_ms.saturating_add(1));
        if let Some((out, source)) = playback {
            out.sample(frame, session, segment, catalog, *source);
        }
        Ok(())
    }

    pub fn all(
        &mut self,
        mut bytes: &[u8],
        mut playback: Option<(&mut Playback, HistorySource)>,
    ) -> HistoryResult<()> {
        while !bytes.is_empty() {
            let len = self.packet(
                bytes,
                playback.as_mut().map(|(out, source)| (&mut **out, *source)),
            )?;
            bytes = &bytes[len..];
        }
        Ok(())
    }
}

/// 只取与视口相交的点及两端邻点，查询结果与解压内存分别设限。
pub(super) struct Playback {
    range: TimeRange,
    samples: HashMap<SampleIdentity, HistorySample>,
    gaps: HashMap<Id, HistoryGap>,
    events: HashMap<Id, HistoryEvent>,
    neighbors: HashMap<(Id, u64), [Option<HistorySample>; 2]>,
    age: HashMap<i32, (Option<TimeRange>, Option<TimeRange>)>,
    bytes: usize,
    truncated: bool,
}

const MAX_POINTS: usize = 262_144;
const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 32_768;
const MAX_NEIGHBOR_SEGMENTS: usize = 4096;

impl Playback {
    pub fn new(range: TimeRange) -> Self {
        Self {
            range,
            samples: HashMap::new(),
            gaps: HashMap::new(),
            events: HashMap::new(),
            neighbors: HashMap::new(),
            age: HashMap::new(),
            bytes: 0,
            truncated: false,
        }
    }

    pub fn prefer_day(&mut self, day: i32, data: Option<TimeRange>, gaps: Option<TimeRange>) {
        self.age.insert(day, (data, gaps));
    }

    fn covered(&self, source: HistorySource, span: TimeRange, is_gap: bool) -> bool {
        let HistorySource::Sealed {
            day,
            current: false,
            ..
        } = source
        else {
            return false;
        };
        self.age.get(&day).is_some_and(|(points, gaps)| {
            let covering = if is_gap { gaps } else { points };
            covering.is_some_and(|r| r.from <= span.from && r.to >= span.to)
        })
    }

    fn sample(
        &mut self,
        frame: Frame,
        session_id: Id,
        segment: u64,
        catalog: &Arc<Catalog>,
        source: HistorySource,
    ) {
        let time = frame.time_ms;
        if self.covered(source, (time, time.saturating_add(1)).into(), false) {
            return;
        }
        let identity = SampleIdentity {
            session_id,
            sample_seq: frame.sample_seq,
        };
        let sample = HistorySample {
            identity,
            frame,
            segment,
            catalog: catalog.clone(),
            source,
        };
        if time < self.range.from || time >= self.range.to {
            let before = time < self.range.from;
            let key = (session_id, segment);
            if !self.neighbors.contains_key(&key) && self.neighbors.len() >= MAX_NEIGHBOR_SEGMENTS {
                self.truncated = true;
                return;
            }
            let slot = &mut self.neighbors.entry(key).or_default()[usize::from(!before)];
            if slot.as_ref().is_none_or(|old| {
                (before && old.frame.time_ms < time) || (!before && old.frame.time_ms > time)
            }) {
                *slot = Some(sample);
            }
            return;
        }
        if self.samples.contains_key(&identity) {
            return;
        }
        let cost = std::mem::size_of::<HistorySample>() * 2
            + sample.frame.values.len() * std::mem::size_of::<Option<u64>>();
        if self.samples.len() >= MAX_POINTS || self.bytes.saturating_add(cost) > MAX_QUERY_BYTES {
            self.truncated = true;
            return;
        }
        self.bytes += cost;
        self.samples.insert(identity, sample);
    }

    fn gap(&mut self, gap: Gap, source: HistorySource) {
        let span = (gap.from_ms, gap.to_ms).into();
        if self.covered(source, span, true) || !self.range.intersects(span) {
            return;
        }
        if self.gaps.len() >= MAX_EVENTS {
            self.truncated = true;
            return;
        }
        self.gaps
            .entry(gap.event_id)
            .or_insert(HistoryGap { gap, source });
    }

    fn event(&mut self, event: Event, session_id: Id, source: HistorySource) {
        if let Some(time) = event.clock.wall_ms
            && (time < self.range.from.saturating_sub(DAY_MS)
                || time >= self.range.to.saturating_add(DAY_MS))
        {
            return;
        }
        if self.events.len() >= MAX_EVENTS {
            self.truncated = true;
            return;
        }
        self.events.entry(event.event_id).or_insert(HistoryEvent {
            event,
            session_id,
            source,
        });
    }

    pub fn finish(mut self, result: &mut HistoryQuery) {
        for sample in self.neighbors.into_values().flatten().flatten() {
            self.samples.entry(sample.identity).or_insert(sample);
        }
        result.samples = self.samples.into_values().collect();
        result.samples.sort_unstable_by_key(|s| {
            (
                s.frame.time_ms,
                s.segment,
                s.identity.session_id,
                s.identity.sample_seq,
            )
        });
        result.gaps = self.gaps.into_values().collect();
        result
            .gaps
            .sort_unstable_by_key(|g| (g.gap.from_ms, g.gap.to_ms, g.gap.event_id));
        result.events = self.events.into_values().collect();
        result
            .events
            .sort_unstable_by_key(|e| (e.event.clock.wall_ms, e.event.segment, e.event.event_id));
        result.truncated |= self.truncated;
    }
}

pub(super) fn page_bytes(page: &PacketPage, after: u64, through: u64) -> HistoryResult<Vec<u8>> {
    if page.offset != after
        || page.through != through
        || after > page.next_offset
        || page.next_offset > through
        || (after != through && page.next_offset == after)
    {
        return Err(HistoryError::Protocol("分页游标不连续或未前进"));
    }
    let len = page.next_offset - after;
    if len > PACKET_PAGE_BUDGET as u64 || page.bytes.len() > 4 * PACKET_PAGE_BUDGET.div_ceil(3) {
        return Err(HistoryError::Limit("包分页长度"));
    }
    let bytes = BASE64_STANDARD
        .decode(&page.bytes)
        .map_err(|_| HistoryError::Protocol("包分页不是有效的 Base64"))?;
    if bytes.len() as u64 != len {
        return Err(HistoryError::Protocol("包分页长度与游标不符"));
    }
    Ok(bytes)
}
