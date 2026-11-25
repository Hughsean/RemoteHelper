use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(crate) const SERIES: [(&str, u32); 3] =
    [("CPU", 0x57d7e2), ("内存", 0xb79aff), ("GPU", 0xf185bb)];

pub(crate) fn series_color(index: usize) -> Hsla {
    rgb(SERIES[index].1).into()
}

#[derive(Clone, Copy)]
struct Sample {
    at: u64,
    values: [Option<f32>; 3],
}

#[derive(Clone, Copy)]
struct PlotPoint {
    at: u64,
    value: f32,
}

#[derive(Clone, Copy)]
struct Timeline {
    from: f64,
    to: f64,
    started: Instant,
    duration: Duration,
}

impl Timeline {
    fn at(self, now: Instant) -> f64 {
        if self.duration.is_zero() {
            return self.to;
        }
        let fraction = (now.saturating_duration_since(self.started).as_secs_f64()
            / self.duration.as_secs_f64())
        .min(1.);
        self.from + (self.to - self.from) * fraction
    }

    fn resting(at: f64, now: Instant) -> Self {
        Self {
            from: at,
            to: at,
            started: now,
            duration: Duration::ZERO,
        }
    }

    fn advance(self, target: u64, now: Instant, interval: Duration) -> Self {
        Self {
            from: self.at(now),
            to: target as f64,
            started: now,
            duration: interval,
        }
    }
}

pub(crate) struct TrendPlot {
    history: VecDeque<Sample>,
    curves: std::rc::Rc<[Vec<Vec<PlotPoint>>; 3]>,
    timeline: Option<Timeline>,
    retention_ms: u64,
    range_ms: u64,
    interval_ms: Option<u64>,
    visible: [bool; 3],
}

impl TrendPlot {
    pub(crate) fn new(range_ms: u64, retention_ms: u64) -> Self {
        Self {
            history: VecDeque::new(),
            curves: Default::default(),
            timeline: None,
            retention_ms,
            range_ms,
            interval_ms: Some(1000),
            visible: [true; 3],
        }
    }

    pub(crate) fn record(&mut self, at: u64, values: [Option<f32>; 3], cx: &mut Context<Self>) {
        let Some(interval_ms) = self.interval_ms else {
            return;
        };
        let now = Instant::now();
        if let Some(last) = self.history.back() {
            if last.at > at {
                self.history.clear();
                self.timeline = None;
            } else if last.at == at {
                self.history.pop_back();
            }
        }

        // Keep one sample before the left edge so the curve can cross it.
        let cutoff = at.saturating_sub(self.retention_ms + interval_ms * 2);
        while self.history.get(1).is_some_and(|sample| sample.at < cutoff) {
            self.history.pop_front();
        }
        self.history.push_back(Sample {
            at,
            values: values.map(|value| {
                value
                    .filter(|value| value.is_finite())
                    .map(|value| value.clamp(0., 100.) / 100.)
            }),
        });
        self.timeline = Some(match self.timeline {
            Some(timeline) if timeline.to < at as f64 => {
                // Keep a small playback buffer to absorb sampling and network jitter.
                timeline.advance(at, now, Duration::from_millis(interval_ms + 100))
            }
            Some(timeline) => timeline,
            None => Timeline::resting(at as f64, now),
        });
        self.rebuild(now);
        cx.notify();
    }

    pub(crate) fn set_interval(&mut self, interval_ms: Option<u64>, cx: &mut Context<Self>) {
        let was_paused = self.interval_ms.is_none();
        self.interval_ms = interval_ms;
        let now = Instant::now();
        if let Some(timeline) = self.timeline {
            if interval_ms.is_none() {
                let at = if cx.reduce_motion() {
                    timeline.to
                } else {
                    timeline.at(now)
                };
                self.timeline = Some(Timeline::resting(at, now));
            } else if let (true, Some(milliseconds), Some(latest)) =
                (was_paused, interval_ms, self.history.back())
            {
                self.timeline =
                    Some(timeline.advance(latest.at, now, Duration::from_millis(milliseconds)));
            }
        }
        cx.notify();
    }

    pub(crate) fn set_range(&mut self, range_ms: u64, cx: &mut Context<Self>) {
        self.range_ms = range_ms;
        self.rebuild(Instant::now());
        cx.notify();
    }

    pub(crate) fn set_visible(&mut self, index: usize, visible: bool, cx: &mut Context<Self>) {
        self.visible[index] = visible;
        cx.notify();
    }

    fn rebuild(&mut self, now: Instant) {
        let end = self.timeline.map_or(0, |timeline| timeline.at(now) as u64);
        let cutoff = end.saturating_sub(self.range_ms);
        let first = self
            .history
            .partition_point(|sample| sample.at < cutoff)
            .saturating_sub(1);
        self.curves = std::rc::Rc::new(std::array::from_fn(|index| {
            let points: Vec<_> = self
                .history
                .range(first..)
                .map(|sample| {
                    sample.values[index].map(|value| PlotPoint {
                        at: sample.at,
                        value,
                    })
                })
                .collect();
            let bucket = points.len().div_ceil(480).max(1);
            points
                .split(Option::is_none)
                .filter(|run| !run.is_empty())
                .map(|run| compact(run, bucket))
                .collect()
        }));
    }
}

fn compact(run: &[Option<PlotPoint>], bucket: usize) -> Vec<PlotPoint> {
    // Preserve local minima and maxima when a long time window has more samples than pixels.
    if bucket == 1 {
        return run.iter().flatten().copied().collect();
    }
    let mut result = Vec::new();
    let Some(first) = run.first().copied().flatten() else {
        return result;
    };
    result.push(first);
    for group in run.chunks(bucket) {
        let mut low = None::<PlotPoint>;
        let mut high = None::<PlotPoint>;
        for point in group.iter().flatten() {
            if low.is_none_or(|extreme| point.value < extreme.value) {
                low = Some(*point);
            }
            if high.is_none_or(|extreme| point.value >= extreme.value) {
                high = Some(*point);
            }
        }
        if let (Some(low), Some(high)) = (low, high) {
            let extremes = if low.at < high.at {
                [low, high]
            } else {
                [high, low]
            };
            for point in extremes {
                if result.last().is_none_or(|last| last.at != point.at) {
                    result.push(point);
                }
            }
        }
    }
    if let Some(last) = run.last().copied().flatten()
        && result.last().is_none_or(|point| point.at != last.at)
    {
        result.push(last);
    }
    result
}

impl Render for TrendPlot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reduced_motion = cx.reduce_motion();
        let end = self.timeline.map_or(0., |timeline| {
            if reduced_motion && self.interval_ms.is_some() {
                timeline.to
            } else {
                timeline.at(now)
            }
        });
        let moving = !reduced_motion
            && self.interval_ms.is_some()
            && self.timeline.is_some_and(|timeline| end < timeline.to)
            && self.visible.iter().any(|visible| *visible);
        if moving && window.is_visible() {
            window.request_animation_frame();
        }
        let range = self.range_ms as f64;
        let start = end - range;
        let curves = self.curves.clone();
        let visible = self.visible;
        let grid = cx.theme().border;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let mut grid_lines = PathBuilder::stroke(px(1.));
                for index in 0..=4 {
                    let fraction = index as f32 / 4.;
                    let x = bounds.left() + bounds.size.width * fraction;
                    let y = bounds.top() + bounds.size.height * fraction;
                    grid_lines.move_to(point(bounds.left(), y));
                    grid_lines.line_to(point(bounds.right(), y));
                    grid_lines.move_to(point(x, bounds.top()));
                    grid_lines.line_to(point(x, bounds.bottom()));
                }
                if let Ok(path) = grid_lines.build() {
                    window.paint_path(path, grid);
                }

                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for (index, runs) in curves.iter().enumerate() {
                        if visible[index] {
                            paint_curve(runs, start, end, bounds, series_color(index), window);
                        }
                    }
                });
            },
        )
        .size_full()
    }
}

fn paint_curve(
    runs: &[Vec<PlotPoint>],
    start: f64,
    end: f64,
    bounds: Bounds<Pixels>,
    color: Hsla,
    window: &mut Window,
) {
    let position = |at: f64, value: f32| {
        point(
            bounds.left() + bounds.size.width * ((at - start) / (end - start)) as f32,
            bounds.bottom() - bounds.size.height * value,
        )
    };

    let mut path = PathBuilder::stroke(px(1.75));
    for run in runs {
        let first = run
            .partition_point(|point| (point.at as f64) < start)
            .saturating_sub(1);
        let last = (run.partition_point(|point| (point.at as f64) <= end) + 1).min(run.len());
        if run.len() == 1 {
            let sample = run[0];
            if (start..=end).contains(&(sample.at as f64)) {
                let center = position(sample.at as f64, sample.value);
                window.paint_quad(
                    fill(
                        Bounds::new(center - point(px(2.), px(2.)), size(px(4.), px(4.))),
                        color,
                    )
                    .corner_radii(px(2.)),
                );
            }
        }
        if first + 1 >= last {
            continue;
        }
        let point = run[first];
        path.move_to(position(point.at as f64, point.value));
        for index in first..last - 1 {
            let (left, right) = (run[index], run[index + 1]);
            let dt = (right.at - left.at) as f64 / 3.;
            let center = slope(left, right);
            let before = index
                .checked_sub(1)
                .map_or(center, |index| slope(run[index], left));
            let after = run
                .get(index + 2)
                .map_or(center, |next| slope(right, *next));
            let left_control = left.value + (dt * tangent(before, center)) as f32;
            let right_control = right.value - (dt * tangent(center, after)) as f32;
            path.cubic_bezier_to(
                position(right.at as f64, right.value),
                position(left.at as f64 + dt, left_control),
                position(right.at as f64 - dt, right_control),
            );
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn slope(left: PlotPoint, right: PlotPoint) -> f64 {
    f64::from(right.value - left.value) / (right.at - left.at) as f64
}

fn tangent(before: f64, after: f64) -> f64 {
    // Flatten extrema. On monotone runs, the harmonic mean limits overshoot.
    if before * after <= 0. {
        0.
    } else {
        2. * before * after / (before + after)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlotPoint, Timeline, compact};
    use std::time::{Duration, Instant};

    #[test]
    fn late_sample_retargets_without_moving_the_current_frame() {
        let start = Instant::now();
        let interval = Duration::from_secs(1);
        let first = Timeline::resting(1000., start).advance(2000, start, interval);
        let halfway = start + Duration::from_millis(500);
        let second = first.advance(3000, halfway, interval);
        assert_eq!(first.at(halfway), 1500.);
        assert_eq!(second.at(halfway), 1500.);
        assert_eq!(second.at(halfway + interval * 2), 3000.);
    }

    #[test]
    fn compressed_history_keeps_extrema_and_edges_in_time_order() {
        let run = [0.3, 0.9, 0.1, 0.6, 0.4].map(Some);
        let points = run
            .into_iter()
            .enumerate()
            .map(|(at, value)| {
                value.map(|value| PlotPoint {
                    at: at as u64,
                    value,
                })
            })
            .collect::<Vec<_>>();
        let selected: Vec<_> = compact(&points, 5).iter().map(|point| point.at).collect();
        assert_eq!(selected, [0, 1, 2, 4]);
    }
}
