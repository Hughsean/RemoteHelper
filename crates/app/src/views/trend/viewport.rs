//! 趋势图的时间视口：目标窗口、正在显示的窗口，以及两者之间的缓动。
//!
//! 交互语义对齐 Argus 的 history 页（`src-gpui/src/frontend/pages/history/state.rs`）：
//! 缩放以指针下的时刻为不动点，窗口宽度在缓动期间按倍率等速变化（几何插值），
//! 滚轮用 167 ms，跳转或切换范围用 250 ms，拖拽直接更新显示窗口。

use std::time::{Duration, Instant};
use time::{OffsetDateTime, UtcOffset, format_description::FormatItem, macros::format_description};

/// 滚轮缩放与平移的缓动时长。
pub(super) const FAST: Duration = Duration::from_millis(167);
/// 回到默认窗口的缓动时长。
pub(super) const SLOW: Duration = Duration::from_millis(250);
/// 打开界面时显示的窗口宽度。
pub(super) const DEFAULT_SPAN_MS: f64 = 300_000.;

/// 放大到极限时的窗口宽度。
const NARROWEST: f64 = 10_000.;
/// 时间刻度的候选步长。
const TICK_STEPS: [f64; 16] = [
    1_000.,
    2_000.,
    5_000.,
    10_000.,
    15_000.,
    30_000., // 秒
    60_000.,
    120_000.,
    300_000.,
    600_000.,
    900_000.,
    1_800_000., // 分
    3_600_000.,
    7_200_000.,
    10_800_000.,
    21_600_000., // 时
];
const DAY_MS: f64 = 86_400_000.;

#[derive(Clone, Copy)]
struct Tween {
    from: (f64, f64),
    started: Instant,
    duration: Duration,
}

/// 一台时间视口：`view` 是目标窗口，`shown` 是这一帧要画的窗口，`extent` 是已记录的
/// 全部时间，`follow` 表示窗口是否跟着最新读数走，`wanted` 是用户要的窗口宽度。
#[derive(Clone, Copy)]
pub(super) struct Viewport {
    view: (f64, f64),
    shown: (f64, f64),
    extent: (f64, f64),
    wanted: f64,
    default_span: f64,
    live_edge: f64,
    follow: bool,
    tween: Option<Tween>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            view: (0., DEFAULT_SPAN_MS),
            shown: (0., DEFAULT_SPAN_MS),
            extent: (0., 1.),
            wanted: DEFAULT_SPAN_MS,
            default_span: DEFAULT_SPAN_MS,
            live_edge: 1.,
            follow: true,
            tween: None,
        }
    }
}

impl Viewport {
    pub(super) fn shown(&self) -> (f64, f64) {
        self.shown
    }

    pub(super) fn extent(&self) -> (f64, f64) {
        self.extent
    }

    pub(super) fn following(&self) -> bool {
        self.follow
    }

    pub(super) fn live_edge(&self) -> f64 {
        self.live_edge
    }

    /// 当前窗口宽度。
    pub(super) fn span(&self) -> f64 {
        (self.view.1 - self.view.0).max(1.)
    }

    fn length(&self) -> f64 {
        (self.extent.1 - self.extent.0).max(1.)
    }

    /// 最窄窗口：固定 10 秒。我们的历史是秒级原始采样，任何长度都还能看细节；
    /// Argus 按范围取五分之一，是因为它的历史按范围分桶。
    pub(super) fn narrowest(&self) -> f64 {
        NARROWEST.min(self.length()).max(1.)
    }

    /// 记录范围随采样变长；跟随最新时窗口一起前移。
    pub(super) fn extend(&mut self, first: f64, clock: f64) {
        self.extend_history(first, clock, clock);
    }

    pub(super) fn target(&self) -> (f64, f64) {
        self.view
    }

    pub(super) fn extend_history(&mut self, first: f64, last: f64, clock: f64) {
        self.extent = (first, last.max(first + 1.));
        self.live_edge = clock.clamp(self.extent.0 + 1., self.extent.1);
        if self.follow {
            self.anchor();
            if self.tween.is_none() {
                self.shown = self.view;
            }
        } else if self.view.0 < first || self.view.1 > self.extent.1 {
            // 两侧都可能随保留策略收缩。
            self.set_window(self.view.0, self.view.1, Duration::ZERO);
        }
    }

    fn anchor(&mut self) {
        let available = self.live_edge - self.extent.0;
        let span = self.wanted.clamp(NARROWEST.min(available), available);
        self.view = (self.live_edge - span, self.live_edge);
    }

    /// 推进缓动一帧；返回是否还在缓动中。
    pub(super) fn advance(&mut self, now: Instant, motion: bool) -> bool {
        let Some(tween) = self.tween else {
            self.shown = self.view;
            return false;
        };
        if !motion {
            self.shown = self.view;
            self.tween = None;
            return false;
        }
        let progress = (now.saturating_duration_since(tween.started).as_secs_f64()
            / tween.duration.as_secs_f64().max(1e-6))
        .clamp(0., 1.);
        self.shown = zoom_between(tween.from, self.view, ease(progress));
        if progress >= 1. {
            self.shown = self.view;
            self.tween = None;
        }
        self.tween.is_some()
    }

    /// 显示 `a..b`，不再跟随最新。
    pub(super) fn set_window(&mut self, a: f64, b: f64, duration: Duration) {
        self.wanted = (b - a).max(1.);
        let span = self.wanted.clamp(self.narrowest(), self.length());
        let start = a.clamp(self.extent.0, (self.extent.1 - span).max(self.extent.0));
        self.follow = false;
        self.view = (start, start + span);
        self.retarget(duration);
    }

    /// 定格当前帧，保留屏幕上已经显示的区间，包括动画中的中间位置。
    pub(super) fn hold(&mut self) {
        self.view = self.shown;
        self.wanted = self.span();
        self.follow = false;
        self.tween = None;
    }

    /// 保持右端不变，只改宽度。
    pub(super) fn set_span(&mut self, span: f64, duration: Duration) {
        self.wanted = span.clamp(self.narrowest(), self.length());
        self.anchor();
        self.retarget(duration);
    }

    /// 倍率在目标宽度上累积，锚点从屏幕上正在显示的窗口取。
    pub(super) fn zoom_at(&mut self, factor: f64, fraction: f64, duration: Duration) -> bool {
        let span = (self.span() * factor).clamp(self.narrowest(), self.length());
        if (span - self.span()).abs() < 0.5 {
            return false;
        }
        let fraction = fraction.clamp(0., 1.);
        let (a, b) = self.shown;
        let at = a + (b - a) * fraction;
        let start =
            (at - span * fraction).clamp(self.extent.0, (self.extent.1 - span).max(self.extent.0));
        self.set_window(start, start + span, duration);
        true
    }

    /// 平移 `delta` 毫秒。
    pub(super) fn pan(&mut self, delta: f64, duration: Duration) {
        let span = self.span();
        let start =
            (self.view.0 + delta).clamp(self.extent.0, (self.extent.1 - span).max(self.extent.0));
        self.set_window(start, start + span, duration);
    }

    /// 回到跟随最新读数。
    pub(super) fn go_latest(&mut self, duration: Duration) {
        self.follow = true;
        self.anchor();
        self.retarget(duration);
    }

    pub(super) fn set_default_span(&mut self, span: f64) {
        let apply = self.follow && self.wanted == self.default_span;
        self.default_span = span.max(1.);
        if apply {
            self.reset(Duration::ZERO);
        }
    }

    /// 回到打开界面时的窗口宽度，并恢复跟随。
    pub(super) fn reset(&mut self, duration: Duration) {
        self.wanted = self.default_span;
        self.go_latest(duration);
    }

    fn retarget(&mut self, duration: Duration) {
        if duration.is_zero() || self.shown == self.view {
            self.shown = self.view;
            self.tween = None;
        } else {
            self.tween = Some(Tween {
                from: self.shown,
                started: Instant::now(),
                duration,
            });
        }
    }
}

/// 两个窗口之间的一部分：宽度按同一倍率变化，围绕两者共有的那个时刻。
fn zoom_between(from: (f64, f64), to: (f64, f64), e: f64) -> (f64, f64) {
    let (was, will) = (from.1 - from.0, to.1 - to.0);
    let linear = (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e);
    if !(was > 0. && will > 0.) || (will / was - 1.).abs() < 1e-9 {
        return linear;
    }
    // 以 from.0 为参照，避免把大时间戳直接相乘。
    let fixed = (to.0 - from.0) * was / (was - will);
    let scale = (will / was).powf(e);
    let start = from.0 + fixed * (1. - scale);
    let zoomed = (start, start + was * scale);
    if zoomed.0.is_finite() && zoomed.1.is_finite() {
        zoomed
    } else {
        linear
    }
}

/// ease-out cubic：起步快，收尾慢。
fn ease(t: f64) -> f64 {
    1. - (1. - t).powi(3)
}

/// 落在步长整数倍上的刻度时刻。
pub(super) fn tick_times(start: f64, end: f64, width: f32) -> Vec<f64> {
    let target = (f64::from(width) / 96.).clamp(2., 12.);
    let minimum = (end - start) / target;
    let step = TICK_STEPS
        .into_iter()
        .find(|step| *step >= minimum)
        .unwrap_or_else(|| (minimum / DAY_MS).ceil() * DAY_MS);
    // 参考窗口中点选刻度基准；每个刻度标签仍按其自身时刻换算时区。
    let offset = f64::from(local_offset(start + (end - start) / 2.).whole_seconds()) * 1000.;
    let mut at = ((start + offset) / step).ceil() * step - offset;
    let mut times = Vec::new();
    while at < end && times.len() < 16 {
        times.push(at);
        at += step;
    }
    times
}

/// 时间刻度标签：窗口不到十分钟时带秒。
pub(super) fn tick_label(at: f64, span: f64) -> String {
    if span >= DAY_MS {
        date_time(at, false)
    } else {
        clock(at, span < 600_000.)
    }
}

/// 把采样时刻格式化成时钟文字。
pub(super) fn clock(at: f64, with_seconds: bool) -> String {
    static SECONDS: &[FormatItem<'static>] = format_description!("[hour]:[minute]:[second]");
    static MINUTES: &[FormatItem<'static>] = format_description!("[hour]:[minute]");
    let format = if with_seconds { SECONDS } else { MINUTES };
    date(at)
        .and_then(|at| at.format(format).ok())
        .unwrap_or_default()
}

pub(super) fn date_time(at: f64, with_seconds: bool) -> String {
    static SECONDS: &[FormatItem<'static>] =
        format_description!("[month]-[day] [hour]:[minute]:[second]");
    static MINUTES: &[FormatItem<'static>] = format_description!("[month]-[day] [hour]:[minute]");
    let format = if with_seconds { SECONDS } else { MINUTES };
    date(at)
        .and_then(|at| at.format(format).ok())
        .unwrap_or_default()
}

pub(super) fn crosses_day(start: f64, end: f64) -> bool {
    date(start).map(|at| at.date()) != date(end).map(|at| at.date())
}

fn date(at: f64) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(at as i128 * 1_000_000)
        .ok()
        .map(|at| at.to_offset(UtcOffset::local_offset_at(at).unwrap_or(UtcOffset::UTC)))
}

fn local_offset(at: f64) -> UtcOffset {
    date(at).map_or(UtcOffset::UTC, |date| date.offset())
}

#[cfg(test)]
mod tests {
    use super::{FAST, SLOW, Viewport, clock, local_offset, tick_times, zoom_between};

    /// 一个不跟随、窗口为 `last - span .. last` 的视口。
    fn viewport(first: f64, last: f64, span: f64) -> Viewport {
        Viewport {
            view: (last - span, last),
            shown: (last - span, last),
            extent: (first, last),
            wanted: span,
            default_span: super::DEFAULT_SPAN_MS,
            live_edge: last,
            follow: false,
            tween: None,
        }
    }

    /// 让缓动走完。
    fn settle(view: &mut Viewport) {
        view.advance(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            true,
        );
    }

    #[test]
    fn zoom_keeps_the_shared_moment_and_scales_the_width() {
        // 两个窗口都把 250 ms 放在四分之一处：缩放前后它都停在那里。
        let from = (0., 1000.);
        let to = (125., 625.);
        for e in [0., 0.25, 0.5, 0.75, 1.] {
            let (a, b) = zoom_between(from, to, e);
            assert!((b - a - 1000. * 0.5f64.powf(e)).abs() < 1e-6);
            assert!(((250. - a) / (b - a) - 0.25).abs() < 1e-9);
        }
    }

    #[test]
    fn zoom_at_is_anchored_and_clamped() {
        // 十分钟历史、两分钟窗口：窗口 480000..600000。
        let mut view = viewport(0., 600_000., 120_000.);
        // 在 540000 处放大一倍，这个时刻落到窗口中点。
        assert!(view.zoom_at(0.5, 0.5, FAST));
        settle(&mut view);
        let (a, b) = view.shown();
        assert!((a - 510_000.).abs() <= 1. && (b - 570_000.).abs() <= 1.);
        // 反复放大停在最窄窗口，反复缩小停在全部历史。
        for _ in 0..40 {
            view.zoom_at(0.5, 1., FAST);
        }
        settle(&mut view);
        assert_eq!(view.span(), view.narrowest());
        for _ in 0..40 {
            view.zoom_at(2., 1., FAST);
        }
        settle(&mut view);
        assert_eq!(view.span(), 600_000.);
        assert!(view.zoom_at(0.5, 0.5, FAST));
        assert_eq!(view.span(), 300_000.);
    }

    #[test]
    fn rapid_zoom_accumulates_and_keeps_the_visible_pointer_anchor() {
        let mut view = viewport(0., 1_000_000., 300_000.);
        assert!(view.zoom_at(0.5, 0.25, FAST));
        assert!(view.zoom_at(0.5, 0.25, FAST));
        assert_eq!(view.view, (756_250., 831_250.));
        let at = view.shown().0 + 0.75 * (view.shown().1 - view.shown().0);
        assert!(view.zoom_at(0.5, 0.75, FAST));
        settle(&mut view);
        assert_eq!(view.shown().0 + 0.75 * view.span(), at);
        assert!(!view.following());
    }

    #[test]
    fn returning_to_live_eases_from_the_held_window() {
        let mut view = viewport(0., 600_000., 120_000.);
        view.set_window(50_000., 170_000., std::time::Duration::ZERO);
        view.go_latest(SLOW);
        assert_eq!(view.shown(), (50_000., 170_000.));
        settle(&mut view);
        assert_eq!(view.shown(), (480_000., 600_000.));
    }

    #[test]
    fn panning_holds_the_window_and_reset_resumes_following() {
        let mut view = Viewport::default();
        view.extend(0., 600_000.);
        view.pan(-150_000., FAST);
        settle(&mut view);
        view.extend(0., 660_000.);
        assert_eq!(view.shown(), (150_000., 450_000.));
        assert!(!view.following());

        assert!(view.zoom_at(0.5, 0.5, FAST));
        assert_eq!(view.span(), 150_000.);
        view.reset(SLOW);
        settle(&mut view);
        view.extend(0., 700_000.);
        assert_eq!(view.shown(), (400_000., 700_000.));
        assert!(view.following());
    }

    #[test]
    fn following_window_slides_with_the_clock() {
        let mut view = Viewport::default();
        view.extend(0., 60_000.);
        assert!(view.following());
        // 数据不足一个窗口时显示全部。
        assert_eq!(view.shown(), (0., 60_000.));
        view.extend(0., 900_000.);
        let (a, b) = view.shown();
        assert!((b - 900_000.).abs() < 1e-6);
        assert!((b - a - 300_000.).abs() < 1e-6);
    }

    #[test]
    fn ticks_land_on_step_multiples() {
        let times = tick_times(1_234., 61_234., 700.);
        assert!(!times.is_empty());
        let step = times[1] - times[0];
        for at in &times {
            let offset = f64::from(local_offset(31_234.).whole_seconds()) * 1000.;
            assert!(((at + offset) / step).fract().abs() < 1e-9);
        }
        assert!(times.iter().all(|at| *at >= 1_234. && *at < 61_234.));
    }

    #[test]
    fn holding_and_resuming_keeps_the_visible_window_width() {
        let mut view = Viewport::default();
        view.extend(0., 600_000.);
        view.zoom_at(0.5, 0.5, FAST);
        let shown = view.shown();
        view.hold();
        view.extend(0., 660_000.);
        assert_eq!(view.shown(), shown);
        assert!(!view.following());

        view.go_latest(std::time::Duration::ZERO);
        assert_eq!(view.shown(), (360_000., 660_000.));
        assert!(view.following());
    }

    #[test]
    fn rollback_keeps_later_wall_time_navigable_without_following_it() {
        let mut view = Viewport::default();
        view.set_default_span(60_000.);
        view.extend_history(0., 900_000., 420_000.);
        assert_eq!(view.shown(), (360_000., 420_000.));
        assert_eq!(view.extent(), (0., 900_000.));
        view.set_window(750_000., 900_000., std::time::Duration::ZERO);
        view.extend_history(35_000., 840_000., 425_000.);
        assert_eq!(view.shown(), (690_000., 840_000.));
    }

    #[test]
    fn clock_shows_seconds_only_for_short_windows() {
        let at = 1_700_000_000_000.;
        assert_eq!(clock(at, true).split(':').count(), 3);
        assert_eq!(clock(at, false).split(':').count(), 2);
    }
}
