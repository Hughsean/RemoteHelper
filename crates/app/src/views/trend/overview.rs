//! 滑轨概览独立于主图视口，只保留低分辨率的有效覆盖及均值。

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use client::history::{HistoryCache, HistoryError, HistoryQuery};
use common::history_api::{Id, TimeRange};

use super::{
    aggregation::{self, ValidInterval},
    playback::{self, Playback},
    step::StepRun,
};

const DAY_MS: i64 = 86_400_000;
const MIN_QUERY_MS: i64 = 3_600_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Prefix {
    id: Id,
    offset: u64,
    current: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::views) struct OverviewPlan {
    store: Id,
    bucket: u64,
    days: BTreeMap<i32, Vec<Prefix>>,
}

impl OverviewPlan {
    pub(in crate::views) fn new(store: Id, query: &HistoryQuery) -> Option<Self> {
        let local = query.local_extent?;
        let (from, to) = query
            .summary
            .as_ref()
            .and_then(|summary| summary.range)
            .map_or((local.from, local.to), |range| {
                (local.from.min(range.from), local.to.max(range.to))
            });
        let bucket = aggregation::bucket_size(from.abs_diff(to) as f64, 640, usize::MAX);
        let mut days: BTreeMap<i32, Vec<Prefix>> = BTreeMap::new();
        for coverage in &query.coverage {
            if coverage.data_range.is_some() || coverage.gap_range.is_some() {
                days.entry(coverage.day).or_default().push(Prefix {
                    id: coverage.day_id,
                    offset: coverage.cached_through,
                    current: coverage.current,
                });
            }
        }
        for versions in days.values_mut() {
            versions.sort_unstable();
        }
        (!days.is_empty()).then_some(Self {
            store,
            bucket,
            days,
        })
    }

    fn same_day(&self, previous: &Self, day: i32) -> bool {
        self.store == previous.store
            && self.bucket == previous.bucket
            && [day.checked_sub(1), Some(day), day.checked_add(1)]
                .into_iter()
                .flatten()
                .all(|day| self.days.get(&day) == previous.days.get(&day))
    }
}

#[derive(Clone, Copy)]
struct Bin {
    from: i64,
    to: i64,
    weight: u64,
    sum: f64,
}

impl Bin {
    fn with_value(from: i64, to: i64, value: f32) -> Self {
        let weight = from.abs_diff(to);
        Self {
            from,
            to,
            weight,
            sum: f64::from(value) * weight as f64,
        }
    }

    fn combine(&mut self, next: Self) {
        self.from = self.from.min(next.from);
        self.to = self.to.max(next.to);
        self.weight += next.weight;
        self.sum += next.sum;
    }

    fn interval(self) -> ValidInterval {
        ValidInterval {
            from: self.from,
            to: self.to,
            value: (self.sum / self.weight as f64) as f32,
            break_before: false,
        }
    }
}

type Series = BTreeMap<i64, Bin>;
type Bins = [Series; 3];

pub(in crate::views) struct Overview {
    plan: OverviewPlan,
    days: BTreeMap<i32, Arc<Bins>>,
    full: Bins,
}

impl Overview {
    pub(in crate::views) fn store_id(&self) -> Id {
        self.plan.store
    }

    pub(in crate::views) fn plan(&self) -> &OverviewPlan {
        &self.plan
    }

    pub(in crate::views) async fn load(
        cache: HistoryCache,
        plan: OverviewPlan,
        previous: Option<Arc<Self>>,
    ) -> Result<Self, HistoryError> {
        let mut days = BTreeMap::new();
        let mut full = Bins::default();
        for &day in plan.days.keys() {
            let from = i64::from(day) * DAY_MS;
            let saved = previous
                .as_ref()
                .filter(|previous| plan.same_day(&previous.plan, day))
                .and_then(|previous| previous.days.get(&day));
            let bins = match saved {
                Some(saved) => saved.clone(),
                None => {
                    Arc::new(read_day(&cache, (from, from + DAY_MS).into(), plan.bucket).await?)
                }
            };
            for (out, day) in full.iter_mut().zip(bins.iter()) {
                for (&key, &part) in day {
                    out.entry(key)
                        .and_modify(|bin| bin.combine(part))
                        .or_insert(part);
                }
            }
            days.insert(day, bins);
        }
        Ok(Self { plan, days, full })
    }

    pub(super) fn runs(
        &self,
        index: usize,
        current: impl IntoIterator<Item = ValidInterval>,
    ) -> Vec<StepRun> {
        let mut bins = self.full[index].clone();
        for interval in current {
            add(&mut bins, interval, self.plan.bucket, true);
        }
        aggregation::covered_runs(bins.into_values().map(Bin::interval), aggregation::RAW)
    }
}

async fn read_day(
    cache: &HistoryCache,
    range: TimeRange,
    bucket: u64,
) -> Result<Bins, HistoryError> {
    let mut pending = vec![range];
    let mut bins = Bins::default();
    while let Some(range) = pending.pop() {
        let mut query = cache.query_overview(range).await?;
        if query.truncated && range.to - range.from > MIN_QUERY_MS {
            // 密集日期按时间平分，避免只显示首先读到的部分。
            let middle = range.from + (range.to - range.from) / 2;
            pending.push((middle, range.to).into());
            pending.push((range.from, middle).into());
            continue;
        }
        // 实时及待封存尾段由主图已加载的数据补齐。
        query
            .samples
            .retain(|point| matches!(point.source, client::history::HistorySource::Sealed { .. }));
        let playback = Playback::new(query, Some(range));
        let merged = playback::merge(Some(&playback), &VecDeque::new(), range);
        for (out, intervals) in bins.iter_mut().zip(merged.series) {
            for interval in intervals {
                add(out, interval, bucket, false);
            }
        }
    }
    Ok(bins)
}

fn add(bins: &mut Series, interval: ValidInterval, width: u64, uncovered_only: bool) {
    let width = i128::from(width.max(1));
    let mut from = interval.from;
    while from < interval.to {
        let key = i128::from(from).div_euclid(width);
        let to = ((key + 1) * width).min(i128::from(interval.to)) as i64;
        let key = key as i64;
        if let Some(bin) = bins.get_mut(&key) {
            if uncovered_only {
                if from < bin.from {
                    bin.combine(Bin::with_value(from, to.min(bin.from), interval.value));
                }
                if to > bin.to {
                    bin.combine(Bin::with_value(from.max(bin.to), to, interval.value));
                }
            } else {
                bin.combine(Bin::with_value(from, to, interval.value));
            }
        } else {
            bins.insert(key, Bin::with_value(from, to, interval.value));
        }
        from = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::history::LocalCoverage;

    #[test]
    fn overview_plan_uses_all_cached_days_even_when_detail_samples_are_empty() {
        let coverage = [4, 9].map(|day| {
            let from = i64::from(day) * DAY_MS;
            LocalCoverage {
                day,
                day_id: [day as u8; 16],
                cached_through: 1024,
                published_through: None,
                data_range: Some((from, from + 1000).into()),
                gap_range: None,
                durable: true,
                current: false,
            }
        });
        let query = HistoryQuery {
            local_extent: Some((4 * DAY_MS, 9 * DAY_MS + 1000).into()),
            coverage: coverage.into(),
            ..HistoryQuery::default()
        };
        let plan = OverviewPlan::new([0; 16], &query).unwrap();
        assert_eq!(plan.days.keys().copied().collect::<Vec<_>>(), [4, 9]);
    }

    #[test]
    fn older_cached_overview_and_live_tail_render_without_bridging_empty_buckets() {
        let mut full = Bins::default();
        let interval = |from, to, value| ValidInterval {
            from,
            to,
            value,
            break_before: false,
        };
        add(&mut full[0], interval(5, 10, 0.4), 10, false);
        add(&mut full[0], interval(20, 25, 0.6), 10, false);
        let overview = Overview {
            plan: OverviewPlan {
                store: [0; 16],
                bucket: 10,
                days: BTreeMap::new(),
            },
            days: BTreeMap::new(),
            full,
        };
        let runs = overview.runs(
            0,
            [
                interval(20, 25, 1.),
                interval(25, 30, 0.8),
                interval(30, 35, 1.),
            ],
        );
        let at = |time| StepRun::reading_at(&runs, time).map(|reading| reading.value);
        assert_eq!(at(6.), Some(0.4));
        assert_eq!(at(15.), None);
        assert!((at(25.).unwrap() - 0.7).abs() <= f32::EPSILON);
        assert_eq!(at(34.), Some(1.));
        assert_eq!(at(36.), None);
    }
}
