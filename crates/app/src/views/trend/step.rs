use gpui_kit::*;

/// 折桶周期，以及该桶中当前连续有效片段的覆盖时长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MeanCoverage {
    pub(super) bucket_ms: u64,
    pub(super) valid_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StepReading {
    pub(super) value: f32,
    /// 原始阶梯的读数没有均值周期。
    pub(super) mean: Option<MeanCoverage>,
}

#[derive(Clone, Copy)]
pub(super) struct PlotPoint {
    pub(super) at: i64,
    pub(super) reading: StepReading,
}

impl PlotPoint {
    pub(super) fn raw(at: i64, value: f32) -> Self {
        Self {
            at,
            reading: StepReading { value, mean: None },
        }
    }
}

/// 无空缺的一段阶梯；有效区间的结束时刻不包含在段内。
pub(super) struct StepRun {
    points: Vec<PlotPoint>,
    end: i64,
    inclusive_end: bool,
}

impl StepRun {
    pub(super) fn new(points: Vec<PlotPoint>, end: i64) -> Self {
        Self {
            points,
            end,
            inclusive_end: false,
        }
    }

    /// 兼容只有采样时刻、没有尾点周期的输入：保留尾点，但不向未来延伸。
    pub(super) fn include_terminal(runs: &mut Vec<Self>, point: PlotPoint) {
        if let Some(run) = runs.last_mut().filter(|run| run.end == point.at) {
            let last = &run.points[run.points.len() - 1];
            // 尾点没有覆盖权重；若仍在同一桶，保留这一桶已算出的均值。
            let same_bucket = last.reading.mean.is_some_and(|mean| {
                let bucket = i128::from(mean.bucket_ms);
                i128::from(last.at).div_euclid(bucket) == i128::from(point.at).div_euclid(bucket)
            });
            if !same_bucket {
                run.points.push(point);
            }
            run.inclusive_end = true;
        } else {
            runs.push(Self {
                points: vec![point],
                end: point.at,
                inclusive_end: true,
            });
        }
    }

    fn ends_before(&self, at: f64) -> bool {
        let end = self.end as f64;
        end < at || (!self.inclusive_end && end == at)
    }

    pub(super) fn peak_in(runs: &[Self], (start, end): (f64, f64)) -> Option<f32> {
        let first = runs.partition_point(|run| run.ends_before(start));
        runs[first..]
            .iter()
            .take_while(|run| run.points[0].at as f64 <= end)
            .flat_map(|run| {
                // 视口起点前的最后一个样本可能仍覆盖视口左侧。
                let first = run
                    .points
                    .partition_point(|point| point.at as f64 <= start)
                    .saturating_sub(1);
                let last = run.points.partition_point(|point| point.at as f64 <= end);
                run.points[first..last]
                    .iter()
                    .map(|point| point.reading.value)
            })
            .reduce(f32::max)
    }

    pub(super) fn reading_at(runs: &[Self], at: f64) -> Option<StepReading> {
        let run = runs.get(runs.partition_point(|run| run.ends_before(at)))?;
        let index = run.points.partition_point(|point| point.at as f64 <= at);
        Some(run.points[index.checked_sub(1)?].reading)
    }
}

pub(super) fn value_y(bounds: Bounds<Pixels>, value: f32) -> Pixels {
    let inset = px(1.).min(bounds.size.height * 0.5);
    bounds.bottom() - inset - (bounds.size.height - inset * 2.) * value
}

pub(super) fn paint_steps(
    runs: &[StepRun],
    time: (f64, f64),
    bounds: Bounds<Pixels>,
    color: Hsla,
    overview: bool,
    axis_top: f32,
    window: &mut Window,
) {
    let (start, end) = time;
    let position = |at: f64, value: f32| {
        point(
            bounds.left() + bounds.size.width * ((at - start) / (end - start).max(1.)) as f32,
            value_y(bounds, value / axis_top),
        )
    };

    let mut line = PathBuilder::stroke(px(if overview { 1. } else { 1.6 }));
    let mut area = PathBuilder::fill();
    let first_run = runs.partition_point(|run| run.ends_before(start));
    for run in &runs[first_run..] {
        let points = &run.points;
        if points[0].at as f64 > end {
            break;
        }
        let first = points
            .partition_point(|point| point.at as f64 <= start)
            .saturating_sub(1);
        let last = points.partition_point(|point| point.at as f64 <= end);
        let from = (points[first].at as f64).max(start);
        let to = (run.end as f64).min(end);
        let mut value = points[first].reading.value;
        let initial = position(from, value);
        if from == to {
            let radius = px(if overview { 1. } else { 2. });
            window.paint_quad(
                fill(
                    Bounds::new(
                        initial - point(radius, radius),
                        size(radius * 2., radius * 2.),
                    ),
                    color,
                )
                .corner_radii(radius),
            );
            continue;
        }

        line.move_to(initial);
        area.move_to(initial);
        for next in &points[first + 1..last] {
            if next.reading.value != value {
                let corner = position(next.at as f64, value);
                let jump = position(next.at as f64, next.reading.value);
                line.line_to(corner);
                line.line_to(jump);
                area.line_to(corner);
                area.line_to(jump);
                value = next.reading.value;
            }
        }
        let final_point = position(to, value);
        line.line_to(final_point);
        area.line_to(final_point);
        area.line_to(position(to, 0.));
        area.line_to(position(from, 0.));
        area.close();
    }
    if let Ok(path) = area.build() {
        window.paint_path(path, color.opacity(if overview { 0.09 } else { 0.05 }));
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::{PlotPoint, StepRun};

    #[test]
    fn peak_uses_visible_steps_and_the_left_edge_carry_in() {
        let runs = [
            StepRun::new(vec![PlotPoint::raw(10, 0.9)], 20),
            StepRun::new(vec![PlotPoint::raw(25, 0.2), PlotPoint::raw(40, 0.8)], 50),
        ];
        assert_eq!(StepRun::peak_in(&runs, (20., 24.)), None);
        assert_eq!(StepRun::peak_in(&runs, (30., 39.)), Some(0.2));
        assert_eq!(StepRun::peak_in(&runs, (30., 41.)), Some(0.8));
    }

    #[test]
    fn readouts_change_at_the_sample_and_leave_missing_intervals_empty() {
        let runs = [
            StepRun::new(vec![PlotPoint::raw(10, 0.2), PlotPoint::raw(23, 0.8)], 29),
            StepRun::new(vec![PlotPoint::raw(42, 0.4)], 43),
        ];
        let value = |at| StepRun::reading_at(&runs, at).map(|reading| reading.value);
        assert_eq!(value(9.9), None);
        assert_eq!(value(10.), Some(0.2));
        assert_eq!(value(22.9), Some(0.2));
        assert_eq!(value(23.), Some(0.8));
        assert_eq!(value(28.9), Some(0.8));
        assert_eq!(value(29.), None);
        assert_eq!(value(41.9), None);
        assert_eq!(value(42.), Some(0.4));
        assert_eq!(value(42.1), Some(0.4));
        assert_eq!(value(43.), None);
    }
}
