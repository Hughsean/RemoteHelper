//! 根据可视范围和图宽选择时间桶，绘图与悬停共用桶内均值。

use super::{
    Sample,
    step::{PlotPoint, StepRun},
};

const DAY_MS: u64 = 86_400_000;
const BUCKET_SIZES: [u64; 19] = [
    500, 1_000, 2_000, 3_000, 5_000, 10_000, 15_000, 30_000, 60_000, 120_000, 180_000, 300_000,
    600_000, 900_000, 1_800_000, 3_600_000, 7_200_000, 21_600_000, DAY_MS,
];

pub(super) const RAW: u64 = 1;

pub(super) fn bucket_size(span: f64, target: usize, samples: usize) -> u64 {
    if samples <= target {
        return RAW;
    }
    let ideal = (span / target as f64).ceil() as u64;
    BUCKET_SIZES
        .into_iter()
        .find(|bucket| *bucket >= ideal)
        .unwrap_or_else(|| ideal.div_ceil(DAY_MS).saturating_mul(DAY_MS))
}

struct Mean {
    first: u64,
    last: u64,
    sum: f64,
    count: usize,
}

impl Mean {
    fn point(&self) -> PlotPoint {
        PlotPoint {
            at: self.first,
            value: (self.sum / self.count as f64) as f32,
        }
    }
}

pub(super) fn runs<'a>(
    samples: impl IntoIterator<Item = &'a Sample>,
    series: usize,
    bucket: u64,
) -> Vec<StepRun> {
    let mut runs = Vec::new();
    let mut points = Vec::new();
    let mut pending: Option<Mean> = None;
    for sample in samples {
        let Some(value) = sample.values[series] else {
            if let Some(mean) = pending.take() {
                points.push(mean.point());
                runs.push(StepRun::new(std::mem::take(&mut points), Some(sample.at)));
            }
            continue;
        };
        // 使用绝对时间的整数倍划分桶，平移窗口时同一段数据不会重新分组。
        if let Some(mean) = &mut pending
            && mean.first / bucket == sample.at / bucket
        {
            mean.last = sample.at;
            mean.sum += f64::from(value);
            mean.count += 1;
        } else if let Some(previous) = pending.replace(Mean {
            first: sample.at,
            last: sample.at,
            sum: f64::from(value),
            count: 1,
        }) {
            points.push(previous.point());
        }
    }
    if let Some(mean) = pending {
        let point = mean.point();
        points.push(point);
        // 最后一格从首条记录画到末条记录，包括末条记录时刻。
        if mean.last != point.at {
            points.push(PlotPoint {
                at: mean.last,
                ..point
            });
        }
        runs.push(StepRun::new(points, None));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::{RAW, Sample, StepRun, bucket_size, runs};

    #[test]
    fn dense_windows_use_bigger_buckets_and_sparse_windows_keep_raw_samples() {
        assert_eq!(bucket_size(300_000., 350, 300), RAW);
        assert_eq!(bucket_size(300_000., 350, 600), 1_000);
        assert_eq!(bucket_size(3_600_000., 350, 3_600), 15_000);
        assert_eq!(bucket_size(3_600_000., 180, 3_600), 30_000);
    }

    #[test]
    fn averages_stay_within_time_buckets_and_do_not_bridge_missing_values() {
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
            values: [value, None, None],
        });
        let averaged = runs(&samples, 0, 1_000);
        let lookup = |at| StepRun::value_at(&averaged, at);
        assert_eq!(lookup(1_699.), None);
        assert_eq!(lookup(1_700.), Some(0.5));
        assert_eq!(lookup(2_200.), Some(0.5));
        assert_eq!(lookup(2_300.), Some(1.));
        assert_eq!(lookup(2_500.), None);
        assert_eq!(lookup(2_599.), None);
        assert_eq!(lookup(2_600.), Some(0.375));
        assert_eq!(lookup(2_800.), Some(0.375));
        assert_eq!(lookup(2_801.), None);

        let raw = runs(&samples, 0, RAW);
        assert_eq!(StepRun::value_at(&raw, 1_700.), Some(0.25));
        assert_eq!(StepRun::value_at(&raw, 1_800.), Some(0.75));
    }
}
