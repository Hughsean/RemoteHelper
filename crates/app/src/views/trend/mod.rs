//! 监控页的趋势图：滚轮缩放、下方进度条平移、悬停读数。
//!
//! 交互与时机对齐 Argus 的 history 页：视口有目标窗口和正在缓动的窗口
//! （`viewport`），进度条是画布上的「窗口框」（`navigator`），缩放以指针下的
//! 时刻为不动点、并暂停实时跟随。

mod aggregation;
mod axis;
mod navigator;
mod overview;
mod playback;
mod status;
mod step;
mod viewport;

use self::navigator::{Brush, NavigatorExtent};
pub(super) use self::overview::{Overview, OverviewPlan};
pub(super) use self::playback::Playback;
use self::playback::{Band, LivePoint, Mark};
use self::step::{StepReading, StepRun, paint_steps, value_y};
use self::viewport::{FAST, SLOW, Viewport, date_time, tick_label, tick_times};
use client::history::HistoryQuery;
use common::{SystemInfo, history_api::TimeRange};
use gpui_kit::base::StyledExt;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    cell::Cell,
    collections::VecDeque,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const SERIES: [&str; 3] = ["CPU", "内存", "GPU"];

const AXIS_GUTTER: f32 = 46.;

pub(crate) fn series_colors(cx: &App) -> [Hsla; SERIES.len()] {
    let theme = cx.theme();
    [theme.chart_1, theme.chart_2, theme.chart_3]
}

/// 滚轮每像素的缩放比：Argus 的 history 页取 1.0018，一个标准滚轮约 1.24 倍。
const ZOOM_RATE: f64 = 1.0018;
/// Shift+滚轮平移时，多少像素走完一个窗口。
const PAN_PIXELS_PER_WINDOW: f64 = 600.;
/// 保留的采样上限，决定缩到最小时能看到多远的历史。
const MAX_SAMPLES: usize = 200_000;

#[derive(Clone, Copy)]
struct Sample {
    at: u64,
    values: [Option<f32>; 3],
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

/// 曲线已经为哪段窗口构建过，避免每帧重建。
#[derive(Clone, Copy)]
struct Built {
    start: f64,
    end: f64,
    bucket: u64,
}

pub(crate) struct TrendPlot {
    // 保留预览和无采样身份的旧服务端入口。
    history: VecDeque<Sample>,
    live: VecDeque<LivePoint>,
    live_epoch: u64,
    live_extent: Option<(i64, i64)>,
    stored: Option<Playback>,
    stored_anchor: Option<i64>,
    bands: Rc<[Band]>,
    marks: Rc<[Mark]>,
    axis: axis::PercentAxis,
    steps: Rc<[Vec<StepRun>; 3]>,
    /// 进度条上整段历史的概览。
    nav_runs: Option<(usize, Rc<[StepRun]>)>,
    nav_history: Option<Arc<Overview>>,
    timeline: Option<Timeline>,
    view: Viewport,
    interval_ms: Option<u64>,
    visible: [bool; 3],
    /// 指针在画布上的位置（窗口坐标）：竖线钉在这个像素上，时刻每帧按窗口重算。
    hover: Option<(Pixels, Pixels)>,
    drag: Option<Brush>,
    nav_extent: NavigatorExtent,
    nav_cursor: CursorStyle,
    chart_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    nav_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    built: Option<Built>,
}

impl TrendPlot {
    pub(crate) fn new(_: &mut Context<Self>) -> Self {
        Self {
            history: VecDeque::new(),
            live: VecDeque::new(),
            live_epoch: 0,
            live_extent: None,
            stored: None,
            stored_anchor: None,
            bands: Rc::from([]),
            marks: Rc::from([]),
            axis: Default::default(),
            steps: Default::default(),
            nav_runs: None,
            nav_history: None,
            timeline: None,
            view: Viewport::default(),
            interval_ms: Some(1000),
            visible: [true; 3],
            hover: None,
            drag: None,
            nav_extent: NavigatorExtent::default(),
            nav_cursor: CursorStyle::PointingHand,
            chart_bounds: Rc::new(Cell::new(None)),
            nav_bounds: Rc::new(Cell::new(None)),
            built: None,
        }
    }

    pub(crate) fn record(&mut self, at: u64, values: [Option<f32>; 3], cx: &mut Context<Self>) {
        let Some(interval_ms) = self.interval_ms else {
            return;
        };
        let now = Instant::now();
        if let Some(last) = self.history.back() {
            if last.at > at {
                // 服务端重启后时间戳会回退，历史和视口一起重置。
                self.history.clear();
                self.timeline = None;
                self.view = Viewport::default();
                self.nav_extent = NavigatorExtent::default();
                self.drag = None;
                self.nav_cursor = CursorStyle::PointingHand;
            } else if last.at == at {
                self.history.pop_back();
            }
        }
        self.history.push_back(Sample {
            at,
            values: values.map(|value| {
                value
                    .filter(|value| value.is_finite())
                    .map(|value| value.clamp(0., 100.) / 100.)
            }),
        });
        while self.history.len() > MAX_SAMPLES {
            self.history.pop_front();
        }
        self.built = None;
        self.nav_runs = None;
        self.timeline = Some(match self.timeline {
            Some(timeline) if timeline.to < at as f64 => {
                // 留一点播放缓冲，吸收采样与网络抖动。
                timeline.advance(at, now, Duration::from_millis(interval_ms + 100))
            }
            Some(timeline) => timeline,
            None => Timeline::resting(at as f64, now),
        });
        cx.notify();
    }

    pub(crate) fn record_live(
        &mut self,
        info: &SystemInfo,
        values: [Option<f32>; 3],
        cx: &mut Context<Self>,
    ) {
        let Some(interval) = self.interval_ms else {
            return;
        };
        if info.sample_identity.is_none()
            && self.live.is_empty()
            && self
                .stored
                .as_ref()
                .is_none_or(|stored| stored.extent.is_none())
        {
            self.record(info.timestamp, values, cx);
            return;
        }
        let Ok(at) = i64::try_from(info.timestamp) else {
            return;
        };
        let period = info
            .period_ms
            .map(i64::from)
            .filter(|&period| period > 0)
            .unwrap_or_else(|| i64::try_from(interval).unwrap_or(i64::MAX).clamp(1, 60_000));
        let previous = self.live.back();
        if previous.is_some_and(|point| match (point.identity, info.sample_identity) {
            (Some(old), Some(new)) => {
                old.session_id == new.session_id && old.sample_seq >= new.sample_seq
            }
            (None, None) => point.at == at,
            _ => false,
        }) {
            return;
        }
        let rollback = previous.is_some_and(|point| point.at > at);
        let cut = rollback
            || previous.is_some_and(|point| {
                point.identity.map(|id| id.session_id)
                    != info.sample_identity.map(|id| id.session_id)
            });
        if cut {
            self.live_epoch = self.live_epoch.saturating_add(1);
        }
        self.live.push_back(LivePoint {
            at,
            period,
            identity: info.sample_identity,
            values: values.map(|value| {
                value
                    .filter(|v| v.is_finite())
                    .map(|v| v.clamp(0., 100.) / 100.)
            }),
            epoch: self.live_epoch,
            rollback,
        });
        if self.live.len() > MAX_SAMPLES {
            self.live.pop_front();
            self.live_extent =
                self.live
                    .iter()
                    .map(|point| point.at)
                    .fold(None::<(i64, i64)>, |extent, at| {
                        Some(extent.map_or((at, at), |(first, last)| (first.min(at), last.max(at))))
                    });
        } else {
            self.live_extent = Some(
                self.live_extent
                    .map_or((at, at), |(first, last)| (first.min(at), last.max(at))),
            );
        }
        let now = Instant::now();
        self.timeline = Some(match self.timeline {
            Some(timeline) if !rollback && timeline.to <= at as f64 => timeline.advance(
                info.timestamp,
                now,
                Duration::from_millis(interval.saturating_add(100)),
            ),
            _ => Timeline::resting(at as f64, now),
        });
        self.invalidate();
        self.update_extent(self.clock(now, cx.reduce_motion()));
        cx.notify();
    }

    /// 清除持久化历史时保留实时样本。
    pub(crate) fn set_history(&mut self, query: HistoryQuery, cx: &mut Context<Self>) {
        self.nav_history = None;
        self.set_prepared_history(Playback::new(query, self.history_range()), cx);
    }

    pub(super) fn set_overview(&mut self, overview: Option<Arc<Overview>>, cx: &mut Context<Self>) {
        self.nav_history = overview;
        self.nav_runs = None;
        cx.notify();
    }

    pub(super) fn set_prepared_history(&mut self, stored: Playback, cx: &mut Context<Self>) {
        self.stored_anchor = stored
            .latest
            .or_else(|| {
                self.stored_anchor.filter(|at| {
                    stored
                        .extent
                        .is_some_and(|extent| extent.from <= *at && *at <= extent.to)
                })
            })
            .or_else(|| stored.extent.map(|extent| extent.to));
        self.stored = Some(stored);
        self.invalidate();
        self.update_extent(self.clock(Instant::now(), cx.reduce_motion()));
        cx.notify();
    }

    pub(crate) fn set_history_window(&mut self, window_ms: u64, cx: &mut Context<Self>) {
        self.view.set_default_span(window_ms as f64);
        cx.notify();
    }

    /// 后台查询使用目标窗口；缓存解码器另外保留两边相邻段的样本。
    pub(crate) fn history_range(&self) -> Option<TimeRange> {
        self.data_span()?;
        let (a, b) = self.view.target();
        let (shown_a, shown_b) = self.view.shown();
        let bucket = self.bucket_for(a, b, self.chart_width());
        let target = aligned_range(a, b, bucket);
        let shown = aligned_range(
            shown_a,
            shown_b,
            self.bucket_for(shown_a, shown_b, self.chart_width()),
        );
        Some((target.from.min(shown.from), target.to.max(shown.to)).into())
    }

    fn invalidate(&mut self) {
        self.built = None;
        self.nav_runs = None;
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
            } else if let (true, Some(milliseconds), Some(latest)) = (
                was_paused,
                interval_ms,
                self.live
                    .back()
                    .and_then(|point| u64::try_from(point.at).ok())
                    .or_else(|| self.history.back().map(|point| point.at)),
            ) {
                self.timeline =
                    Some(timeline.advance(latest, now, Duration::from_millis(milliseconds)));
            }
        }
        cx.notify();
    }

    pub(crate) fn set_visible(&mut self, index: usize, visible: bool, cx: &mut Context<Self>) {
        self.visible[index] = visible;
        self.nav_runs = None;
        cx.notify();
    }

    /// 正在跟随的那一帧的时刻：采样之间按时间插值，减少动画时直接用目标。
    fn clock(&self, now: Instant, reduced_motion: bool) -> f64 {
        match self.timeline {
            Some(timeline) if reduced_motion => timeline.to,
            Some(timeline) => timeline.at(now),
            None => self
                .stored_anchor
                .map(|at| at as f64)
                .or_else(|| self.history.back().map(|sample| sample.at as f64))
                .unwrap_or(0.),
        }
    }

    fn data_span(&self) -> Option<(f64, f64)> {
        let legacy = self
            .history
            .front()
            .zip(self.history.back())
            .map(|(a, b)| (a.at as f64, b.at as f64));
        let live = self.live_extent.map(|(a, b)| (a as f64, b as f64));
        let stored = self
            .stored
            .as_ref()
            .and_then(|data| data.extent)
            .map(|extent| (extent.from as f64, extent.to as f64));
        [legacy, live, stored]
            .into_iter()
            .flatten()
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }

    fn update_extent(&mut self, clock: f64) {
        if let Some((first, last)) = self.data_span() {
            self.view.extend_history(first, last, clock);
        }
    }

    fn modern(&self) -> bool {
        self.stored
            .as_ref()
            .is_some_and(|stored| stored.extent.is_some())
            || !self.live.is_empty()
    }

    fn interactive(&self) -> bool {
        self.data_span().is_some_and(|(first, last)| last > first)
    }

    fn chart_width(&self) -> f32 {
        self.chart_bounds
            .get()
            .map_or(740., |bounds| f32::from(bounds.size.width))
    }

    fn chart_fraction(&self, x: Pixels) -> Option<f32> {
        let bounds = self.chart_bounds.get()?;
        (bounds.size.width > px(0.))
            .then(|| ((x - bounds.left()) / bounds.size.width).clamp(0., 1.))
    }

    /// 竖线画在哪个位置、读的是哪个时刻。
    fn hover_mapping(&self) -> Option<(f32, f64)> {
        let (x, _) = self.hover?;
        let fraction = self.chart_fraction(x)?;
        let (start, end) = self.view.shown();
        Some((fraction, start + (end - start) * f64::from(fraction)))
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        if !self.interactive()
            || self.drag.is_some()
            || (dy == 0. && (!event.modifiers.shift || dx == 0.))
        {
            return;
        }
        cx.stop_propagation();
        if event.modifiers.shift {
            let span = self.view.span();
            let step = if dx != 0. { dx } else { dy };
            self.view.pan(-span * step / PAN_PIXELS_PER_WINDOW, FAST);
        } else if let Some(fraction) = self.chart_fraction(event.position.x) {
            self.view
                .zoom_at(ZOOM_RATE.powf(-dy), f64::from(fraction), FAST);
        }
        cx.notify();
    }

    fn hover_at(&mut self, x: Pixels, y: Pixels, cx: &mut Context<Self>) {
        if let Some(bounds) = self.chart_bounds.get()
            && x >= bounds.left()
            && x <= bounds.right()
            && y >= bounds.top()
            && y <= bounds.bottom()
        {
            self.hover = Some((x, y));
        } else {
            self.hover = None;
        }
        cx.notify();
    }

    pub(crate) fn clear_hover(&mut self, cx: &mut Context<Self>) {
        if self.hover.take().is_some() {
            cx.notify();
        }
    }

    /// 双击图表：回到默认窗口并恢复跟随。
    fn reset_view(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        self.view.reset(SLOW);
        cx.notify();
    }

    /// 「实时」按钮：回到跟随最新读数。
    fn resume_live(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.view.go_latest(SLOW);
        cx.notify();
    }

    /// 缩放到密集历史时，根据屏幕宽度选时间桶；平移时复用已构建的范围。
    fn bucket_for(&self, a: f64, b: f64, width: f32) -> u64 {
        let target = (width / 2.6).round().clamp(60., 640.) as usize;
        let range: TimeRange = (a.floor() as i64, (b.floor() as i64).saturating_add(1)).into();
        let visible = if self.modern() {
            let stored = self
                .stored
                .as_ref()
                .map_or(0, |stored| stored.visible_count(range));
            let live = self
                .live
                .iter()
                .filter(|point| range.from <= point.at && point.at < range.to)
                .count();
            // 尾段常与实时样本重复，不能把同一段采样密度计算两次。
            stored.max(live)
        } else {
            let first = self
                .history
                .partition_point(|sample| (sample.at as f64) < a);
            let last = self
                .history
                .partition_point(|sample| (sample.at as f64) <= b);
            last.saturating_sub(first)
        };
        aggregation::bucket_size((b - a).max(1.), target, visible)
    }

    fn sync_steps(&mut self, width: f32) {
        let (a, b) = self.view.shown();
        let bucket = self.bucket_for(a, b, width);
        if self
            .built
            .is_none_or(|built| bucket != built.bucket || a < built.start || b > built.end)
        {
            self.rebuild(bucket);
        }
    }

    fn rebuild(&mut self, bucket: u64) {
        let (a, b) = self.view.shown();
        let range = aligned_range(a, b, bucket);
        let (start, end) = (range.from as f64, range.to as f64);
        if self.modern() {
            let merged = playback::merge(self.stored.as_ref(), &self.live, range);
            self.steps = Rc::new(
                merged
                    .series
                    .map(|series| aggregation::covered_runs(series, bucket)),
            );
            self.bands = merged.bands.into();
            self.marks = merged.marks.into();
        } else {
            let first = self
                .history
                .partition_point(|sample| (sample.at as f64) < start)
                .saturating_sub(2);
            let last = (self
                .history
                .partition_point(|sample| (sample.at as f64) <= end)
                + 2)
            .clamp(first, self.history.len());
            self.steps = Rc::new(std::array::from_fn(|index| {
                aggregation::runs(self.history.range(first..last), index, bucket)
            }));
        }
        self.built = Some(Built { start, end, bucket });
    }

    fn sync_overview(&mut self) {
        if self.nav_runs.is_none()
            && let Some((first, last)) = self.data_span()
            && let Some(index) = self.visible.iter().position(|visible| *visible)
        {
            let count = self.history.len()
                + self.live.len()
                + self.stored.as_ref().map_or(0, |stored| stored.count);
            let bucket = aggregation::bucket_size(last - first, 640, count);
            let runs = if self.modern() {
                let extent = (first.floor() as i64, (last.ceil() as i64).saturating_add(1)).into();
                let mut merged = playback::merge(self.stored.as_ref(), &self.live, extent);
                let current = std::mem::take(&mut merged.series[index]);
                if let Some(overview) = &self.nav_history {
                    overview.runs(index, current)
                } else {
                    aggregation::covered_runs(current, bucket)
                }
            } else {
                aggregation::runs(&self.history, index, bucket)
            };
            self.nav_runs = Some((index, runs.into()));
        }
    }

    /// 指针处的读数浮层：时刻 + 各条可见曲线的当前值。
    fn tooltip(
        &self,
        at: f64,
        values: [Option<StepReading>; SERIES.len()],
        annotation: Option<String>,
        cx: &App,
    ) -> Option<AnyElement> {
        let (x, y) = self.hover?;
        let bounds = self.chart_bounds.get()?;
        let colors = series_colors(cx);
        let bucket = self.built.map_or(aggregation::RAW, |built| built.bucket);
        let time = date_time(at, true);
        let header = match bucket {
            aggregation::RAW => time,
            60_000.. => format!("{time} · {} 分钟均值", bucket as f64 / 60_000.),
            _ => format!("{time} · {} 秒均值", bucket as f64 / 1_000.),
        };
        let mut rows = Vec::new();
        rows.push(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(header),
        );
        for (index, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                rows.push(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .text_xs()
                        .child(div().w(px(9.)).h(px(2.)).rounded_full().bg(colors[index]))
                        .child(
                            div()
                                .flex_1()
                                .text_color(cx.theme().muted_foreground)
                                .child(SERIES[index]),
                        )
                        .child(
                            div()
                                .font_semibold()
                                .text_color(cx.theme().foreground)
                                .child(match value.mean {
                                    Some(mean) => format!(
                                        "{:.1}% · 有效 {}",
                                        value.value * 100.,
                                        status::duration(mean.valid_ms)
                                    ),
                                    None => format!("{:.1}%", value.value * 100.),
                                }),
                        ),
                );
            }
        }
        let annotated = annotation.is_some();
        if let Some(annotation) = annotation {
            rows.push(div().text_xs().child(annotation));
        }
        if rows.len() == 1 {
            return None;
        }

        let width = if annotated {
            242.
        } else if bucket == aggregation::RAW {
            136.
        } else {
            212.
        };
        let height = 16. + 18. * rows.len() as f32;
        let chart_width = f32::from(bounds.size.width);
        let chart_height = f32::from(bounds.size.height);
        let pointer_x = f32::from(x - bounds.left());
        let pointer_y = f32::from(y - bounds.top());
        // 靠近右边缘时翻到指针左侧，上下都留在图内。
        let left = if pointer_x + width + 14. > chart_width {
            (pointer_x - width - 14.).max(0.)
        } else {
            pointer_x + 14.
        };
        let top = (pointer_y - height * 0.5).clamp(2., (chart_height - height - 2.).max(2.));

        Some(
            v_flex()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border.opacity(0.65))
                .bg(cx.theme().popover)
                .p_2()
                .gap_1()
                .children(rows)
                .into_any_element(),
        )
    }

    /// 开发用：在图上放一个假的指针位置，核对读数浮层。
    #[cfg(debug_assertions)]
    pub(crate) fn debug_hover(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if let Some(bounds) = self.chart_bounds.get() {
            self.hover = Some((
                bounds.left() + bounds.size.width * x,
                bounds.top() + bounds.size.height * y,
            ));
            cx.notify();
        }
    }

    /// 开发用：把视口缩放到一段历史里，核对冻结态的画面。
    #[cfg(debug_assertions)]
    pub(crate) fn debug_zoom(&mut self, factor: f64, cx: &mut Context<Self>) {
        self.view.zoom_at(factor, 0.35, SLOW);
        cx.notify();
    }

    /// 开发用：灌入一段合成历史，便于离线核对界面。
    #[cfg(debug_assertions)]
    pub(crate) fn seed(
        &mut self,
        last_at: u64,
        step_ms: u64,
        count: usize,
        cx: &mut Context<Self>,
    ) {
        let first = last_at.saturating_sub(step_ms * (count.saturating_sub(1) as u64));
        for index in 0..count {
            let at = first + step_ms * index as u64;
            let phase = index as f32 / 41.;
            let values = [
                Some(35.0 + 28.0 * phase.sin() + 4.0 * (phase * 7.).sin()),
                Some(46.0 + 12.0 * (phase * 0.37).cos()),
                Some(22.0 + 18.0 * (phase * 0.9).sin().abs()),
            ];
            self.history.push_back(Sample {
                at,
                values: values.map(|value| value.map(|value| value.clamp(0., 100.) / 100.)),
            });
        }
        let now = Instant::now();
        self.timeline = Some(Timeline::resting(last_at as f64, now));
        if let Some((first_at, _)) = self.data_span() {
            self.view.extend(first_at, last_at as f64);
        }
        self.built = None;
        self.nav_runs = None;
        cx.notify();
    }
}

/// UTC 整桶范围另留缓冲；余数按欧几里得定义计算，兼顾纪元前时间。
fn aligned_range(a: f64, b: f64, bucket: u64) -> TimeRange {
    let width = i128::from(bucket.max(1));
    let margin = ((b - a) * 0.1).ceil().max(width as f64);
    let from = i128::from((a - margin).floor() as i64);
    let to = i128::from((b + margin).ceil() as i64);
    let limit = |value: i128| value.clamp(i64::MIN.into(), i64::MAX.into()) as i64;
    (
        limit(from.div_euclid(width) * width),
        limit((to - 1).div_euclid(width) * width + width),
    )
        .into()
}

#[cfg(test)]
mod tests {
    use super::aligned_range;

    #[test]
    fn history_query_covers_full_utc_buckets_and_carry_in_margin() {
        let range = aligned_range(-1_050.25, 18_300.75, 5_000);
        assert_eq!(range, (-10_000, 25_000).into());
        assert_eq!(range.from.rem_euclid(5_000), 0);
        assert_eq!(range.to.rem_euclid(5_000), 0);
    }
}

impl Render for TrendPlot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reduced_motion = cx.reduce_motion();
        let clock = self.clock(now, reduced_motion);
        self.update_extent(clock);
        let tweening = self.view.advance(now, !reduced_motion);
        let flowing = self.interval_ms.is_some()
            && !reduced_motion
            && self.timeline.is_some_and(|timeline| clock < timeline.to);
        let navigating = self.drag.is_none()
            && self.data_span().is_some()
            && self
                .nav_extent
                .advance(self.view.extent(), now, !reduced_motion);
        let chart_bounds = self.chart_bounds.clone();
        let measured_width = self.chart_width();
        self.sync_steps(measured_width);
        self.sync_overview();
        let (start, end) = self.view.shown();
        let span = (end - start).max(1.);
        let visible = self.visible;
        let peak = self
            .steps
            .iter()
            .zip(visible)
            .filter(|(_, show)| *show)
            .filter_map(|(runs, _)| StepRun::peak_in(runs, (start, end)))
            .reduce(f32::max);
        let (axis_top, axis_marks, axis_moving) = self.axis.update(peak, now, !reduced_motion);
        if (tweening || flowing || navigating || axis_moving) && window.is_visible() {
            window.request_animation_frame();
        }
        let grid_marks = axis_marks.clone();
        let theme = cx.theme();
        let grid = theme.chart_grid;
        let marker_edge = theme.background.blend(theme.group_box);
        let colors = series_colors(cx);
        let muted = cx.theme().muted_foreground;
        let crosshair = muted.opacity(0.7);
        let chart = cx.weak_entity();
        let has_data = self.data_span().is_some();
        let hover = self
            .hover_mapping()
            .filter(|_| self.drag.is_none() && has_data);
        let hover_values = hover.map_or([None; SERIES.len()], |(_, at)| {
            std::array::from_fn(|index| {
                visible[index]
                    .then(|| StepRun::reading_at(&self.steps[index], at))
                    .flatten()
            })
        });
        let ticks: Vec<_> = if !has_data {
            Vec::new()
        } else {
            tick_times(start, end, measured_width)
                .into_iter()
                .map(|at| (((at - start) / span) as f32, tick_label(at, span)))
                .collect()
        };
        let label_inset = (45. / measured_width.max(1.)).min(0.5);
        let tick_lines: Vec<_> = ticks.iter().map(|(fraction, _)| *fraction).collect();
        let sync = self.stored.as_ref().map(|stored| &stored.status);
        let annotation = hover.and_then(|(_, at)| {
            status::annotation(
                &self.bands,
                &self.marks,
                at,
                span * 5. / f64::from(measured_width.max(1.)),
                sync,
            )
        });
        let tooltip = hover.and_then(|(_, at)| self.tooltip(at, hover_values, annotation, cx));
        let steps = self.steps.clone();
        let bands = self.bands.clone();
        let marks = self.marks.clone();
        let status_colors = status::Colors::new(cx);
        let legend = self.stored.as_ref().map(|stored| {
            status::legend(
                &self.bands,
                &self.marks,
                (start, end),
                &stored.status,
                stored.truncated,
                status_colors,
                muted,
            )
        });
        let empty = if !has_data && self.stored.is_some() {
            Some("此主机没有可用历史数据")
        } else if !has_data {
            Some("等待采样数据")
        } else if !visible.contains(&true) {
            Some("选择指标以显示趋势")
        } else {
            None
        };

        v_flex()
            .size_full()
            .pt_2()
            .gap_1p5()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        div()
                            .relative()
                            .w(px(AXIS_GUTTER))
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(muted)
                            .children(axis_marks.into_iter().map(|(value, at, alpha)| {
                                div()
                                    .absolute()
                                    .right(px(9.))
                                    .top(relative(at.max(0.)))
                                    .mt(px(-7.))
                                    .line_height(px(14.))
                                    .text_color(muted.opacity(alpha))
                                    .child(format!("{value}%"))
                            })),
                    )
                    .child(
                        div()
                            .id("trend-plot")
                            .relative()
                            .min_w_0()
                            .flex_1()
                            .when(empty.is_none(), |plot| plot.cursor_crosshair())
                            .on_scroll(cx.listener(Self::on_scroll))
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                this.hover_at(event.position.x, event.position.y, cx)
                            }))
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if !*hovered {
                                    this.clear_hover(cx);
                                }
                            }))
                            .on_mouse_exit({
                                let plot = cx.entity().downgrade();
                                move |_: &MouseExitEvent, _, cx| {
                                    if let Some(plot) = plot.upgrade() {
                                        plot.update(cx, |this, cx| this.clear_hover(cx));
                                    }
                                }
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    if this.interactive() {
                                        this.view.hold();
                                        cx.notify();
                                    }
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    if this.interactive() && !this.view.following() {
                                        this.view.go_latest(SLOW);
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let previous = chart_bounds.replace(Some(bounds));
                                        if previous.is_none_or(|previous| {
                                            previous.size.width != bounds.size.width
                                        }) {
                                            window.request_animation_frame();
                                        }
                                    },
                                    move |bounds, _, window, _| {
                                        if hover.is_some() {
                                            let chart = chart.clone();
                                            window.on_mouse_event(
                                                move |event: &MouseMoveEvent, phase, _, cx| {
                                                    if phase == DispatchPhase::Capture
                                                        && !bounds.contains(&event.position)
                                                    {
                                                        let _ = chart.update(cx, Self::clear_hover);
                                                    }
                                                },
                                            );
                                        }
                                        if bounds.size.width <= px(1.)
                                            || bounds.size.height <= px(1.)
                                        {
                                            return;
                                        }
                                        window.with_content_mask(
                                            Some(ContentMask { bounds }),
                                            |window| {
                                                status::paint_bands(
                                                    &bands,
                                                    (start, end),
                                                    bounds,
                                                    status_colors,
                                                    window,
                                                );
                                                for &(_, at, alpha) in &grid_marks {
                                                    let y = bounds.top()
                                                        + bounds.size.height * at.max(0.);
                                                    let mut rule = PathBuilder::stroke(px(1.));
                                                    rule.move_to(point(bounds.left(), y));
                                                    rule.line_to(point(bounds.right(), y));
                                                    if let Ok(path) = rule.build() {
                                                        window
                                                            .paint_path(path, grid.opacity(alpha));
                                                    }
                                                }
                                                let mut rules = PathBuilder::stroke(px(1.));
                                                for fraction in &tick_lines {
                                                    let x = bounds.left()
                                                        + bounds.size.width * *fraction;
                                                    rules.move_to(point(x, bounds.top()));
                                                    rules.line_to(point(x, bounds.bottom()));
                                                }
                                                if let Ok(path) = rules.build() {
                                                    window.paint_path(path, grid);
                                                }
                                                for (index, runs) in steps.iter().enumerate() {
                                                    if visible[index] {
                                                        paint_steps(
                                                            runs,
                                                            (start, end),
                                                            bounds,
                                                            colors[index],
                                                            false,
                                                            axis_top,
                                                            window,
                                                        );
                                                    }
                                                }
                                                status::paint_marks(
                                                    &marks,
                                                    (start, end),
                                                    bounds,
                                                    status_colors,
                                                    window,
                                                );
                                                if let Some((fraction, _)) = hover {
                                                    let x = bounds.left()
                                                        + bounds.size.width * fraction;
                                                    let mut line = PathBuilder::stroke(px(1.))
                                                        .dash_array(&[px(3.), px(3.)]);
                                                    line.move_to(point(x, bounds.top()));
                                                    line.line_to(point(x, bounds.bottom()));
                                                    if let Ok(path) = line.build() {
                                                        window.paint_path(path, crosshair);
                                                    }
                                                    for (index, value) in
                                                        hover_values.into_iter().enumerate()
                                                    {
                                                        if let Some(value) = value {
                                                            let y = value_y(
                                                                bounds,
                                                                value.value / axis_top,
                                                            );
                                                            window.paint_quad(quad(
                                                                Bounds::new(
                                                                    point(x - px(4.), y - px(4.)),
                                                                    size(px(8.), px(8.)),
                                                                ),
                                                                px(4.),
                                                                colors[index],
                                                                px(1.5),
                                                                marker_edge,
                                                                BorderStyle::Solid,
                                                            ));
                                                        }
                                                    }
                                                }
                                            },
                                        );
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            )
                            .children(empty.map(|text| {
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(text)
                            }))
                            .children(tooltip),
                    ),
            )
            .child(
                h_flex()
                    .h(px(20.))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(div().w(px(AXIS_GUTTER)).flex_shrink_0())
                    .child(
                        div().relative().flex_1().min_w_0().h_full().children(
                            ticks
                                .into_iter()
                                .filter(|(fraction, _)| {
                                    (label_inset..=1. - label_inset).contains(fraction)
                                })
                                .map(|(fraction, label)| {
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left(relative(fraction))
                                        .ml(px(-45.))
                                        .w(px(90.))
                                        .text_center()
                                        .line_height(px(17.))
                                        .child(label)
                                }),
                        ),
                    ),
            )
            .children(legend)
            .child(self.navigator(cx))
            .children(self.drag.is_some().then(|| self.navigator_drag_capture(cx)))
    }
}
