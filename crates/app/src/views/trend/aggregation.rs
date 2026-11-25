//! 根据可视范围和图宽选择时间桶，绘图与悬停共用有效区间的时间加权均值。

use super::{
    Sample,
    step::{MeanCoverage, PlotPoint, StepReading, StepRun},
};

const DAY_MS: u64 = 86_400_000;
const BUCKET_SIZES: [u64; 19] = [
    500, 1_000, 2_000, 3_000, 5_000, 10_000, 15_000, 30_000, 60_000, 120_000, 180_000, 300_000,
    600_000, 900_000, 1_800_000, 3_600_000, 7_200_000, 21_600_000, DAY_MS,
];

pub(super) const RAW: u64 = 1;

pub(super) fn bucket_size(span: f64, target: usize, samples: usize) -> u64 {
    let target = target.max(1);
    if samples <= target {
        return RAW;
    }
    let ideal = (span / target as f64).ceil() as u64;
    BUCKET_SIZES
        .into_iter()
        .find(|bucket| *bucket >= ideal)
        .unwrap_or_else(|| ideal.div_ceil(DAY_MS).saturating_mul(DAY_MS))
}

/// 单通道的有效阶梯，区间为 UTC 毫秒的半开区间 `[from, to)`。
#[derive(Clone, Copy)]
pub(super) struct ValidInterval {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) value: f32,
    /// 即使与上一段相接也要断线，例如回拨后切换到另一个时钟段。
    pub(super) break_before: bool,
}

struct Mean {
    from: i64,
    valid_ms: u64,
    weighted_sum: f64,
}

impl Mean {
    fn point(self, bucket_ms: u64) -> PlotPoint {
        PlotPoint {
            at: self.from,
            reading: StepReading {
                value: (self.weighted_sum / self.valid_ms as f64) as f32,
                mean: Some(MeanCoverage {
                    bucket_ms,
                    valid_ms: self.valid_ms,
                }),
            },
        }
    }
}

/// 输入按起点升序排列、互不重叠。调用方先合并来源、确定有界尾部并扣除缺口与缺值，
/// 再传入查询涉及的完整桶中的有效区间；画图时才裁到视口，平移不会改变桶内读数。
pub(super) fn covered_runs(
    intervals: impl IntoIterator<Item = ValidInterval>,
    bucket: u64,
) -> Vec<StepRun> {
    let bucket = bucket.max(RAW);
    let mut runs = Vec::new();
    let mut points = Vec::new();
    let mut mean: Option<Mean> = None;
    let mut end = None;
    for interval in intervals {
        if interval.from >= interval.to || !interval.value.is_finite() {
            continue;
        }
        if let Some(previous_end) = end {
            debug_assert!(previous_end <= interval.from, "有效区间必须有序且互不重叠");
            if previous_end != interval.from || interval.break_before {
                if let Some(mean) = mean.take() {
                    points.push(mean.point(bucket));
                }
                runs.push(StepRun::new(std::mem::take(&mut points), previous_end));
            }
        }
        end = Some(interval.to);
        if bucket == RAW {
            points.push(PlotPoint::raw(interval.from, interval.value));
            continue;
        }

        // 使用绝对 UTC 边界切分权重；扩大中间整数类型，兼顾纪元前和整数边界。
        let mut from = interval.from;
        let width = i128::from(bucket);
        while from < interval.to {
            let start = i128::from(from);
            let boundary = start + width - start.rem_euclid(width);
            let to = boundary.min(i128::from(interval.to)) as i64;
            let valid_ms = from.abs_diff(to);
            let weighted_sum = f64::from(interval.value) * valid_ms as f64;
            let pending = mean.get_or_insert(Mean {
                from,
                valid_ms: 0,
                weighted_sum: 0.,
            });
            pending.valid_ms += valid_ms;
            pending.weighted_sum += weighted_sum;
            if i128::from(to) == boundary {
                points.push(mean.take().unwrap().point(bucket));
            }
            from = to;
        }
    }
    if let Some(end) = end {
        if let Some(mean) = mean {
            points.push(mean.point(bucket));
        }
        runs.push(StepRun::new(points, end));
    }
    runs
}

/// 旧实时采样不含周期或持久化缺口，只能持有到相邻采样，尾点不向未来延伸。
pub(super) fn runs<'a>(
    samples: impl IntoIterator<Item = &'a Sample>,
    series: usize,
    bucket: u64,
) -> Vec<StepRun> {
    let mut samples = samples
        .into_iter()
        .filter_map(|sample| i64::try_from(sample.at).ok().map(|at| (at, sample)))
        .peekable();
    let mut terminal = None;
    let intervals = std::iter::from_fn(|| {
        while let Some((at, sample)) = samples.next() {
            let Some(value) = sample.values[series].filter(|value| value.is_finite()) else {
                continue;
            };
            let Some(&(to, _)) = samples.peek() else {
                terminal = Some(PlotPoint::raw(at, value));
                return None;
            };
            return Some(ValidInterval {
                from: at,
                to,
                value,
                break_before: false,
            });
        }
        None
    });
    let mut runs = covered_runs(intervals, bucket);
    if let Some(point) = terminal {
        StepRun::include_terminal(&mut runs, point);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(from: i64, to: i64, value: f32) -> ValidInterval {
        ValidInterval {
            from,
            to,
            value,
            break_before: false,
        }
    }

    fn mean(value: f32, bucket_ms: u64, valid_ms: u64) -> Option<StepReading> {
        Some(StepReading {
            value,
            mean: Some(MeanCoverage {
                bucket_ms,
                valid_ms,
            }),
        })
    }

    #[test]
    fn dense_windows_use_bigger_buckets_and_sparse_windows_keep_raw_samples() {
        assert_eq!(bucket_size(300_000., 350, 300), RAW);
        assert_eq!(bucket_size(300_000., 350, 600), 1_000);
        assert_eq!(bucket_size(3_600_000., 350, 3_600), 15_000);
        assert_eq!(bucket_size(3_600_000., 180, 3_600), 30_000);
    }

    #[test]
    fn ten_half_second_samples_and_one_five_second_sample_have_equal_weight() {
        let intervals = (0..10)
            .map(|i| interval(i * 500, (i + 1) * 500, 1.))
            .chain([interval(5_000, 10_000, 0.)]);
        let runs = covered_runs(intervals, 10_000);
        assert_eq!(StepRun::reading_at(&runs, 0.), mean(0.5, 10_000, 10_000));
        assert_eq!(
            StepRun::reading_at(&runs, 9_999.5),
            mean(0.5, 10_000, 10_000)
        );
        assert_eq!(StepRun::reading_at(&runs, 10_000.), None);
    }

    #[test]
    fn split_weights_at_absolute_buckets_and_keep_them_stable_across_query_ranges() {
        let intervals = [
            interval(1_500, 1_750, 0.5),
            interval(1_750, 2_250, 1.),
            interval(2_250, 3_500, 0.),
        ];
        let runs = covered_runs(intervals, 1_000);
        assert_eq!(StepRun::reading_at(&runs, 1_499.), None);
        assert_eq!(StepRun::reading_at(&runs, 1_999.5), mean(0.75, 1_000, 500));
        assert_eq!(StepRun::reading_at(&runs, 2_000.), mean(0.25, 1_000, 1_000));
        assert_eq!(StepRun::reading_at(&runs, 3_000.), mean(0., 1_000, 500));

        let narrower = covered_runs(intervals[1..].iter().copied(), 1_000);
        assert_eq!(
            StepRun::reading_at(&narrower, 2_600.),
            StepRun::reading_at(&runs, 2_600.)
        );
    }

    #[test]
    fn same_bucket_gap_and_clock_segment_boundary_keep_separate_stairs() {
        let resolved = [
            interval(0, 100, 0.),
            interval(100, 300, 0.75),
            interval(500, 600, 1.),
            ValidInterval {
                break_before: true,
                ..interval(600, 700, 0.25)
            },
            interval(700, 850, 0.25),
        ];
        let runs = covered_runs(resolved, 1_000);
        assert_eq!(runs.len(), 3);
        let read = |at| StepRun::reading_at(&runs, at);
        assert_eq!(read(299.5), mean(0.5, 1_000, 300));
        assert_eq!(read(300.), None);
        assert_eq!(read(499.5), None);
        assert_eq!(read(500.), mean(1., 1_000, 100));
        assert_eq!(read(600.), mean(0.25, 1_000, 250));
        assert_eq!(read(850.), None);
    }

    #[test]
    fn raw_stairs_keep_fractional_changes_and_explicit_half_open_tail() {
        let runs = covered_runs([interval(-300, -100, 0.25), interval(-100, 20, 0.75)], RAW);
        let value = |at| StepRun::reading_at(&runs, at).map(|reading| reading.value);
        assert_eq!(value(-300.), Some(0.25));
        assert_eq!(value(-100.5), Some(0.25));
        assert_eq!(value(-100.), Some(0.75));
        assert_eq!(StepRun::reading_at(&runs, 19.).unwrap().mean, None);
        assert_eq!(value(20.), None);
    }

    #[test]
    fn utc_bucket_alignment_uses_floor_before_the_epoch() {
        let runs = covered_runs([interval(-600, -250, 1.), interval(-250, 250, 0.)], 500);
        assert_eq!(StepRun::reading_at(&runs, -550.), mean(1., 500, 100));
        assert_eq!(StepRun::reading_at(&runs, -500.), mean(0.5, 500, 500));
        assert_eq!(StepRun::reading_at(&runs, 0.), mean(0., 500, 250));
    }

    #[test]
    fn legacy_samples_are_weighted_and_sensor_gaps_only_affect_their_channel() {
        let samples = [
            (1_700, Some(0.25)),
            (1_800, Some(0.75)),
            (2_300, Some(1.)),
            (2_500, None),
            (2_600, Some(0.25)),
            (2_800, Some(0.5)),
        ]
        .map(|(at, value)| Sample {
            at,
            values: [value, Some(0.5), None],
        });
        let averaged = runs(&samples, 0, 1_000);
        let read = |at| StepRun::reading_at(&averaged, at);
        assert_eq!(read(1_700.), mean(7. / 12., 1_000, 300));
        assert_eq!(read(2_000.), mean(0.85, 1_000, 500));
        assert_eq!(read(2_500.), None);
        assert_eq!(read(2_599.5), None);
        assert_eq!(read(2_600.), mean(0.25, 1_000, 200));
        assert_eq!(read(2_800.), mean(0.25, 1_000, 200));
        assert_eq!(read(2_800.1), None);
        assert_eq!(
            StepRun::reading_at(&runs(&samples, 1, RAW), 2_500.).map(|reading| reading.value),
            Some(0.5)
        );

        let raw = runs(&samples, 0, RAW);
        let value = |at| StepRun::reading_at(&raw, at).map(|reading| reading.value);
        assert_eq!(value(1_700.), Some(0.25));
        assert_eq!(value(1_800.), Some(0.75));
        assert_eq!(value(2_800.), Some(0.5));
        assert_eq!(value(2_801.), None);
    }
}
