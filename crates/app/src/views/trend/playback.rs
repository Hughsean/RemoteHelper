//! 合并历史来源和墙钟重叠，产出供阶梯和状态底纹共同使用的半开区间。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use client::history::{HistoryQuery, HistorySample, HistorySource};
use common::{
    SampleIdentity,
    history_api::{DirectoryEntry, Id, TimeRange},
};
use history::{GapReason, MarkerKind};

use super::aggregation::ValidInterval;

const DAY_MS: i64 = 86_400_000;
const LIVE: u8 = 0;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct LivePoint {
    pub(super) at: i64,
    pub(super) identity: Option<SampleIdentity>,
    pub(super) period: i64,
    pub(super) values: [Option<f32>; 3],
    pub(super) epoch: u64,
    pub(super) rollback: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Segment(u64, u64);

#[derive(Clone, Copy)]
struct Point {
    at: i64,
    identity: Option<SampleIdentity>,
    period: i64,
    values: [Option<f32>; 3],
    segment: Segment,
    source: u8,
}

fn source_rank(source: HistorySource) -> u8 {
    match source {
        HistorySource::Sealed { current: true, .. } => 3,
        HistorySource::Sealed { current: false, .. } => 2,
        HistorySource::Tail { .. } => 1,
    }
}

impl From<&HistorySample> for Point {
    fn from(sample: &HistorySample) -> Self {
        let mut cells = [None; 4];
        for (column, value) in sample.catalog.channels.iter().zip(&sample.frame.values) {
            let slot = match column.key.as_str() {
                "cpu.usage" => 0,
                "mem.used" => 1,
                "mem.total" => 2,
                "gpu.usage" => 3,
                _ => continue,
            };
            cells[slot] = value.and_then(|value| column.dequantize(value));
        }
        let percent = |v: f64| (v / 100.0).clamp(0., 1.) as f32;
        Self {
            at: sample.frame.time_ms,
            identity: Some(sample.identity),
            period: i64::from(sample.frame.period_ms),
            values: [
                cells[0].map(percent),
                cells[1]
                    .zip(cells[2])
                    .filter(|&(_, total)| total > 0.)
                    .map(|(used, total)| (used / total).clamp(0., 1.) as f32),
                cells[3].map(percent),
            ],
            segment: Segment(sample.segment, 0),
            source: source_rank(sample.source),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BandKind {
    Gap { reason: GapReason, duration: u64 },
    Expired { until: Option<i64> },
    Sync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Band {
    pub(super) range: TimeRange,
    pub(super) kind: BandKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mark {
    pub(super) at: i64,
    pub(super) rollback: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    Sample([Option<f32>; 3]),
    Band(BandKind),
    Clear,
}

/// 后台返回的是已验证的数据；将传感器目录和设备值转为图表使用的紧凑表示。
pub(in crate::views) struct Playback {
    points: Vec<Point>,
    gaps: Vec<Evidence>,
    states: Vec<Evidence>,
    sealed_coverage: Vec<TimeRange>,
    marks: Vec<Mark>,
    pub(super) extent: Option<TimeRange>,
    pub(super) latest: Option<i64>,
    pub(super) count: usize,
    pub(super) status: client::history::SyncStatus,
    pub(super) truncated: bool,
}

#[derive(Clone, Copy)]
struct Evidence {
    range: TimeRange,
    // 已确认的数据先按段号处理重叠，同段内优先明确缺口、再按来源选择。
    rank: (u8, Segment, u8, u8),
    kind: Kind,
}

fn extend(extent: &mut Option<TimeRange>, range: Option<TimeRange>) {
    if let Some(range) = range.filter(|range| range.from < range.to) {
        *extent = Some(extent.map_or(range, |old| {
            (old.from.min(range.from), old.to.max(range.to)).into()
        }));
    }
}

impl Playback {
    pub(in crate::views) fn new(query: HistoryQuery, scope: Option<TimeRange>) -> Self {
        let mut extent = query.summary.as_ref().and_then(|summary| summary.range);
        extend(&mut extent, query.local_extent);
        let latest_id = query.summary.as_ref().and_then(|summary| summary.latest);
        let mut states = Vec::new();
        let cutoff = query
            .directories
            .first()
            .map(|directory| directory.age_cutoff_day)
            .unwrap_or_else(|| {
                query
                    .summary
                    .as_ref()
                    .and_then(|summary| summary.age_cutoff_day)
            })
            .map(|day| i64::from(day) * DAY_MS);
        let mut status_band = |range, kind| {
            let rank = if matches!(kind, BandKind::Expired { .. }) {
                1
            } else {
                0
            };
            states.push(Evidence {
                range,
                rank: (rank, Segment::default(), 0, 0),
                kind: Kind::Band(kind),
            });
        };
        if let Some(cutoff) = cutoff {
            // 保留边界不将可导航范围无限向过去扩张；本地数据仍可穿过此边界。
            status_band(
                (i64::MIN, cutoff).into(),
                BandKind::Expired {
                    until: Some(cutoff),
                },
            );
        }
        let day_range = |day| (i64::from(day) * DAY_MS, (i64::from(day) + 1) * DAY_MS).into();
        let local: HashMap<_, _> = query
            .coverage
            .iter()
            .map(|entry| ((entry.day, entry.day_id), entry.cached_through))
            .collect();
        let mut sealed_coverage = Vec::new();
        let mut days = HashSet::new();
        // 较新的目录快照优先，不用旧代号误报同步或过期。
        for entry in query
            .directories
            .iter()
            .flat_map(|directory| &directory.entries)
        {
            let day = match entry {
                DirectoryEntry::Available(day) => day.day,
                DirectoryEntry::Pending(day) | DirectoryEntry::Evicted(day) => day.day,
                DirectoryEntry::Unavailable { day, .. } => *day,
            };
            if !days.insert(day) {
                continue;
            }
            match entry {
                DirectoryEntry::Available(entry) => {
                    for range in [entry.data_range, entry.gap_range].into_iter().flatten() {
                        extend(&mut extent, Some(range));
                        if local.get(&(day, entry.day_id)).copied().unwrap_or(0)
                            < entry.published_through
                        {
                            status_band(range, BandKind::Sync);
                        }
                    }
                    if !query.truncated
                        && local.get(&(day, entry.day_id)).copied().unwrap_or(0)
                            >= entry.published_through
                        && let Some(range) = entry.data_range
                    {
                        let range = scope.map_or(range, |scope| {
                            (range.from.max(scope.from), range.to.min(scope.to)).into()
                        });
                        if range.from < range.to {
                            sealed_coverage.push(range);
                        }
                    }
                }
                DirectoryEntry::Pending(entry) | DirectoryEntry::Evicted(entry) => {
                    for range in [entry.data_range, entry.gap_range].into_iter().flatten() {
                        status_band(range, BandKind::Expired { until: None });
                    }
                }
                DirectoryEntry::Unavailable { .. } => status_band(day_range(day), BandKind::Sync),
            }
        }
        // 目录尚未取得时，只在摘要指示可能有历史、且本次尚未确认的区间显示待取底纹。
        if let Some(summary) = query.summary.as_ref().and_then(|summary| summary.range) {
            let mut known: Vec<_> = query
                .directories
                .iter()
                .map(|directory| directory.range)
                .collect();
            known.sort_unstable_by_key(|range| range.from);
            let mut from = summary.from;
            for range in known {
                if from < range.from.min(summary.to) {
                    status_band((from, range.from.min(summary.to)).into(), BandKind::Sync);
                }
                from = from.max(range.to);
            }
            if from < summary.to {
                status_band((from, summary.to).into(), BandKind::Sync);
            }
        }
        if query.truncated
            && let Some(scope) = scope
        {
            status_band(scope, BandKind::Sync);
        }
        for local in &query.coverage {
            extend(&mut extent, local.data_range);
            extend(&mut extent, local.gap_range);
        }

        let mut points: Vec<_> = query.samples.iter().map(Point::from).collect();
        points.sort_unstable_by_key(|point| point.at);
        let mut segments = BTreeMap::<u64, i64>::new();
        let mut latest = query
            .summary
            .as_ref()
            .and_then(|summary| summary.latest_wall_ms);
        for point in &points {
            segments
                .entry(point.segment.0)
                .and_modify(|v| *v = (*v).max(point.at))
                .or_insert(point.at);
            extend(
                &mut extent,
                Some((point.at, point.at.saturating_add(point.period)).into()),
            );
            if point.identity == latest_id {
                latest = Some(point.at);
            }
        }
        // 相邻查询带入的样本不应把离线锚点移到当前范围边缘。
        if latest.is_none() && latest_id.is_none() {
            latest = segments.last_key_value().map(|(_, at)| *at);
        }
        let represented: HashSet<_> = points.iter().filter_map(|point| point.identity).collect();
        let mut marks = BTreeMap::<i64, bool>::new();
        for entry in query.events {
            let event = entry.event;
            if event.kind != MarkerKind::ClockSegment {
                continue;
            }
            let Some(at) = event.clock.wall_ms else {
                continue;
            };
            if !matches!(
                entry.source,
                HistorySource::Sealed { current: true, .. } | HistorySource::Tail { .. }
            ) && !event.sample_seq.is_some_and(|sample_seq| {
                represented.contains(&SampleIdentity {
                    session_id: entry.session_id,
                    sample_seq,
                })
            }) {
                continue;
            }
            let rollback = segments
                .range(..event.segment)
                .next_back()
                .is_some_and(|(_, previous)| *previous > at);
            marks
                .entry(at)
                .and_modify(|value| *value |= rollback)
                .or_insert(rollback);
        }

        let mut seen = HashSet::<Id>::new();
        let mut gaps = query.gaps;
        gaps.sort_by_key(|gap| std::cmp::Reverse(source_rank(gap.source)));
        let gaps = gaps
            .into_iter()
            .filter_map(|entry| {
                let gap = entry.gap;
                if !seen.insert(gap.event_id) || gap.from_ms >= gap.to_ms {
                    return None;
                }
                let range = (gap.from_ms, gap.to_ms).into();
                extend(&mut extent, Some(range));
                let duration = gap.from_ms.abs_diff(gap.to_ms);
                Some(Evidence {
                    range,
                    rank: (
                        3,
                        Segment(gap.segment, 0),
                        1 + reason_rank(gap.reason),
                        source_rank(entry.source),
                    ),
                    kind: Kind::Band(BandKind::Gap {
                        reason: gap.reason,
                        duration,
                    }),
                })
            })
            .collect();
        Self {
            extent,
            latest,
            status: query.status,
            count: points.len(),
            truncated: query.truncated,
            points,
            gaps,
            states,
            sealed_coverage,
            marks: marks
                .into_iter()
                .map(|(at, rollback)| Mark { at, rollback })
                .collect(),
        }
    }

    pub(super) fn visible_count(&self, range: TimeRange) -> usize {
        let first = self.points.partition_point(|point| point.at < range.from);
        let last = self.points.partition_point(|point| point.at < range.to);
        last - first
    }
}

fn reason_rank(reason: GapReason) -> u8 {
    match reason {
        GapReason::Disabled => 7,
        GapReason::Storage | GapReason::RecorderOverrun => 6,
        GapReason::ClockJump | GapReason::Rebooted => 5,
        GapReason::Stopped | GapReason::Interrupted => 4,
        GapReason::Sleep => 3,
        GapReason::Stalled => 2,
        GapReason::Unknown => 1,
    }
}

#[derive(Default)]
pub(super) struct Merged {
    pub(super) series: [Vec<ValidInterval>; 3],
    pub(super) bands: Vec<Band>,
    pub(super) marks: Vec<Mark>,
}

pub(super) fn merge(
    stored: Option<&Playback>,
    live: &VecDeque<LivePoint>,
    range: TimeRange,
) -> Merged {
    let mut evidence = Vec::new();
    let mut marks = BTreeMap::<i64, bool>::new();
    let mut by_id = HashMap::<SampleIdentity, Point>::new();
    if let Some(stored) = stored {
        for &point in &stored.points {
            if let Some(id) = point.identity {
                by_id
                    .entry(id)
                    .and_modify(|old| {
                        if point.source > old.source {
                            *old = point;
                        }
                    })
                    .or_insert(point);
            }
        }
        evidence.extend(
            stored
                .gaps
                .iter()
                .chain(&stored.states)
                .filter(|item| item.range.intersects(range))
                .copied(),
        );
        marks.extend(
            stored
                .marks
                .iter()
                .filter(|mark| range.from <= mark.at && mark.at < range.to)
                .map(|mark| (mark.at, mark.rollback)),
        );
    }
    let mut sessions = HashMap::<Id, Vec<(u64, Segment)>>::new();
    for (&id, point) in &by_id {
        sessions
            .entry(id.session_id)
            .or_default()
            .push((id.sample_seq, point.segment));
    }
    for session in sessions.values_mut() {
        session.sort_unstable();
    }
    let mut epochs = HashMap::<u64, Segment>::new();
    for point in live {
        if let Some(stored) = point.identity.and_then(|id| by_id.get(&id)) {
            epochs
                .entry(point.epoch)
                .and_modify(|old| *old = (*old).max(stored.segment))
                .or_insert(stored.segment);
        }
    }
    let mut usable_live = Vec::new();
    let mut previous = None::<(u64, Segment)>;
    for &point in live {
        let persisted = point.identity.and_then(|id| by_id.get(&id));
        let segment = persisted
            .map(|point| point.segment)
            .or_else(|| {
                previous
                    .filter(|prev| prev.0 == point.epoch)
                    .map(|prev| prev.1)
            })
            .or_else(|| epochs.get(&point.epoch).copied())
            .or_else(|| {
                let id = point.identity?;
                let session = sessions.get(&id.session_id)?;
                let i = session.partition_point(|(seq, _)| *seq <= id.sample_seq);
                let base = session.get(i.saturating_sub(1))?.1;
                Some(if point.rollback {
                    Segment(base.0, point.epoch)
                } else {
                    base
                })
            })
            .unwrap_or(Segment(0, point.epoch));
        previous = Some((point.epoch, segment));
        if point.rollback && range.from <= point.at && point.at < range.to {
            marks.insert(point.at, true);
        }
        if persisted.is_some() {
            continue;
        }
        let item = Point {
            at: point.at,
            identity: point.identity,
            period: point.period,
            segment,
            values: point.values,
            source: LIVE,
        };
        if let Some(id) = point.identity {
            by_id.insert(id, item);
        } else {
            usable_live.push(item);
        }
    }
    let mut points: Vec<_> = by_id.into_values().chain(usable_live).collect();
    points.sort_unstable_by_key(|point| {
        (
            point.segment,
            point.at,
            point.source,
            point.identity.map(|id| (id.session_id, id.sample_seq)),
        )
    });
    // 同段同毫秒只能拥有一个可见阶梯；相同来源时后写入的采样优先。
    points.dedup_by(|later, earlier| {
        if later.segment == earlier.segment && later.at == earlier.at {
            *earlier = *later;
            true
        } else {
            false
        }
    });
    // 较低优先级实时样本不能提前截断已验证的阶梯。
    let mut persisted_next = vec![None; points.len()];
    let mut successor: Option<Point> = None;
    for (i, &point) in points.iter().enumerate().rev() {
        persisted_next[i] = successor.filter(|next| next.segment == point.segment);
        if point.source != LIVE {
            successor = Some(point);
        }
    }
    let mut gaps = BTreeMap::<Segment, Vec<TimeRange>>::new();
    for item in &evidence {
        if matches!(item.kind, Kind::Band(BandKind::Gap { .. })) {
            gaps.entry(item.rank.1).or_default().push(item.range);
        }
    }
    for list in gaps.values_mut() {
        list.sort_unstable_by_key(|gap| gap.from);
        let mut union = Vec::<TimeRange>::new();
        for gap in list.drain(..) {
            if let Some(previous) = union.last_mut().filter(|previous| previous.to >= gap.from) {
                previous.to = previous.to.max(gap.to);
            } else {
                union.push(gap);
            }
        }
        *list = union;
    }
    for (i, &point) in points.iter().enumerate() {
        if point.at >= range.to {
            continue;
        }
        let nominal = point.at.saturating_add(point.period.max(1));
        let next = if point.source == LIVE {
            points
                .get(i + 1)
                .copied()
                .filter(|next| next.segment == point.segment)
        } else {
            persisted_next[i]
        };
        let threshold = (3 * point.period.max(next.map_or(0, |next| next.period))).max(10_000);
        let mut end = match next {
            Some(next) if next.at.saturating_sub(point.at) <= threshold => next.at,
            Some(next) => nominal.min(next.at),
            None => nominal,
        };
        if let Some(list) = gaps.get(&point.segment) {
            let start = list.partition_point(|gap| gap.to <= point.at);
            if let Some(gap) = list.get(start).filter(|gap| gap.from < end) {
                end = end.min(nominal).min(gap.from);
            }
        }
        if end <= range.from || end <= point.at {
            continue;
        }
        evidence.push(Evidence {
            range: (point.at, end).into(),
            rank: (
                if point.source == LIVE { 2 } else { 3 },
                point.segment,
                0,
                point.source,
            ),
            kind: Kind::Sample(point.values),
        });
    }
    // 已发布水位之前，实时数据不能填补封存包已确认的留白。
    for item in stored
        .into_iter()
        .flat_map(|stored| &stored.sealed_coverage)
    {
        if item.intersects(range) {
            evidence.push(Evidence {
                range: *item,
                rank: (2, Segment(u64::MAX, u64::MAX), 0, 0),
                kind: Kind::Clear,
            });
        }
    }
    let mut merged = sweep(evidence, range);
    merged.marks = marks
        .into_iter()
        .map(|(at, rollback)| Mark { at, rollback })
        .collect();
    merged
}

fn sweep(evidence: Vec<Evidence>, range: TimeRange) -> Merged {
    let mut edges = Vec::with_capacity(evidence.len() * 2);
    for (index, item) in evidence.iter().enumerate() {
        if item.range.intersects(range) {
            edges.push((item.range.from.max(range.from), true, index));
            edges.push((item.range.to.min(range.to), false, index));
        }
    }
    edges.sort_unstable();
    let mut active = BTreeSet::new();
    let mut merged = Merged::default();
    let mut last = [None::<Segment>; 3];
    let mut start = range.from;
    let mut selected = None;
    let mut edges = edges.into_iter().peekable();
    // 只在获胜证据变化处切片，低优先级边界不会影响桶权重。
    while let Some(&(at, _, _)) = edges.peek() {
        while let Some((_, add, index)) = edges.next_if(|event| event.0 == at) {
            let key = (evidence[index].rank, index);
            if add {
                active.insert(key);
            } else {
                active.remove(&key);
            }
        }
        let next = active.last().map(|&(_, index)| index);
        if next != selected {
            emit(&evidence, selected, &mut merged, &mut last, start, at);
            selected = next;
            start = at;
        }
    }
    emit(&evidence, selected, &mut merged, &mut last, start, range.to);
    merged
}

fn emit(
    evidence: &[Evidence],
    selected: Option<usize>,
    merged: &mut Merged,
    last: &mut [Option<Segment>; 3],
    from: i64,
    to: i64,
) {
    if from >= to {
        return;
    }
    match selected.map(|index| evidence[index]) {
        Some(Evidence {
            rank: (_, segment, _, _),
            kind: Kind::Sample(values),
            ..
        }) => {
            for (i, value) in values.into_iter().enumerate() {
                if let Some(value) = value {
                    merged.series[i].push(ValidInterval {
                        from,
                        to,
                        value,
                        break_before: last[i] != Some(segment),
                    });
                    last[i] = Some(segment);
                } else {
                    last[i] = None;
                }
            }
        }
        Some(Evidence {
            kind: Kind::Band(kind),
            ..
        }) => {
            if let Some(band) = merged
                .bands
                .last_mut()
                .filter(|band| band.range.to == from && band.kind == kind)
            {
                band.range.to = to;
            } else {
                merged.bands.push(Band {
                    range: (from, to).into(),
                    kind,
                });
            }
            *last = [None; 3];
        }
        Some(Evidence {
            kind: Kind::Clear, ..
        })
        | None => *last = [None; 3],
    }
}
