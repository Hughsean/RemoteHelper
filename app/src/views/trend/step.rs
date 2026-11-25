use gpui_kit::*;

#[derive(Clone, Copy)]
pub(super) struct PlotPoint {
    pub(super) at: u64,
    pub(super) value: f32,
}

/// 一段有读数的采样。读数从采样时刻保持到下次采样，缺失时结束当前段。
pub(super) struct StepRun {
    points: Vec<PlotPoint>,
    missing_at: Option<u64>,
}

impl StepRun {
    pub(super) fn new(points: Vec<PlotPoint>, missing_at: Option<u64>) -> Self {
        Self { points, missing_at }
    }

    fn end(&self) -> f64 {
        self.missing_at
            .unwrap_or(self.points[self.points.len() - 1].at) as f64
    }

    fn ends_before(&self, at: f64) -> bool {
        let end = self.end();
        end < at || (self.missing_at.is_some() && end == at)
    }

    pub(super) fn value_at(runs: &[Self], at: f64) -> Option<f32> {
        let run = runs.get(runs.partition_point(|run| run.ends_before(at)))?;
        let index = run.points.partition_point(|point| point.at as f64 <= at);
        Some(run.points[index.checked_sub(1)?].value)
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
    window: &mut Window,
) {
    let (start, end) = time;
    let position = |at: f64, value: f32| {
        point(
            bounds.left() + bounds.size.width * ((at - start) / (end - start).max(1.)) as f32,
            value_y(bounds, value),
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
        let to = run.end().min(end);
        let mut value = points[first].value;
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
            if next.value != value {
                let corner = position(next.at as f64, value);
                let jump = position(next.at as f64, next.value);
                line.line_to(corner);
                line.line_to(jump);
                area.line_to(corner);
                area.line_to(jump);
                value = next.value;
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
    fn readouts_change_at_the_sample_and_leave_missing_intervals_empty() {
        let runs = [
            StepRun::new(
                vec![
                    PlotPoint { at: 10, value: 0.2 },
                    PlotPoint { at: 23, value: 0.8 },
                ],
                Some(29),
            ),
            StepRun::new(vec![PlotPoint { at: 42, value: 0.4 }], None),
        ];
        let value = |at| StepRun::value_at(&runs, at);
        assert_eq!(value(9.9), None);
        assert_eq!(value(10.), Some(0.2));
        assert_eq!(value(22.9), Some(0.2));
        assert_eq!(value(23.), Some(0.8));
        assert_eq!(value(28.9), Some(0.8));
        assert_eq!(value(29.), None);
        assert_eq!(value(41.9), None);
        assert_eq!(value(42.), Some(0.4));
        assert_eq!(value(42.1), None);
    }
}
