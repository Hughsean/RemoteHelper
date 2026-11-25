//! 滑块绘制整段历史。框体平移，双侧手柄分别调整起点和终点。

use super::{
    TrendPlot, series_colors,
    step::paint_steps,
    viewport::{SLOW, Viewport, clock, crosses_day, date_time},
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

const EDGE_PX: f32 = 7.;
const MIN_FRAME_PX: f32 = 10.;

impl TrendPlot {
    pub(super) fn navigator(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let extent = self.drag.map_or(self.view.extent(), |drag| drag.extent);
        let shown = self.view.shown();
        let following = self.view.following();
        let has_data = self.data_span().is_some();
        let interactive = self.interactive();
        let with_date = crosses_day(extent.0, extent.1);
        let with_seconds = extent.1 - extent.0 < 600_000.;
        let label = |at| {
            if !has_data {
                "—".into()
            } else if with_date {
                date_time(at, false)
            } else {
                clock(at, with_seconds)
            }
        };

        let nav_bounds = self.nav_bounds.clone();
        let overview = self.nav_runs.clone();
        let colors = series_colors(cx);
        let shade = theme.background.opacity(0.54);
        let frame_color = theme
            .primary
            .opacity(if self.drag.is_some() { 1. } else { 0.88 });

        let strip = div()
            .id("trend-navigator")
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(22.))
            .cursor(if interactive {
                self.nav_cursor
            } else {
                CursorStyle::Arrow
            })
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.drag.is_none() {
                    let cursor = this
                        .brush_at(event.position.x)
                        .map_or(CursorStyle::PointingHand, Handle::cursor);
                    if this.nav_cursor != cursor {
                        this.nav_cursor = cursor;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if !this.interactive() {
                        return;
                    }
                    if event.click_count >= 2 {
                        this.reset_view(window, cx);
                    } else {
                        this.navigator_brush_down(event.position.x);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| nav_bounds.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        let width = f32::from(bounds.size.width);
                        if width <= 1. || !has_data {
                            return;
                        }
                        let top = bounds.top();
                        let height = bounds.size.height;
                        let box_range = brush_frame(extent, shown, width);
                        let left = bounds.left() + px(box_range.0);
                        let right = bounds.left() + px(box_range.1);

                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            if let Some((index, runs)) = &overview {
                                let plot = Bounds::new(
                                    point(bounds.left(), top + px(3.)),
                                    size(bounds.size.width, height - px(6.)),
                                );
                                paint_steps(
                                    runs,
                                    extent,
                                    plot,
                                    colors[*index].opacity(0.78),
                                    true,
                                    window,
                                );
                            }
                            if !interactive {
                                return;
                            }
                            for (x, w) in [
                                (bounds.left(), left - bounds.left()),
                                (right, bounds.right() - right),
                            ] {
                                if w > px(0.) {
                                    window.paint_quad(fill(
                                        Bounds::new(point(x, top), size(w, height)),
                                        shade,
                                    ));
                                }
                            }
                            window.paint_quad(quad(
                                Bounds::new(point(left, top), size(right - left, height)),
                                px(4.),
                                frame_color.opacity(0.10),
                                px(1.),
                                frame_color,
                                BorderStyle::Solid,
                            ));
                            for handle in [left + px(1.5), right - px(1.5)] {
                                let grip = Bounds::new(
                                    point(handle - px(1.5), top + (height - px(10.)) / 2.),
                                    size(px(3.), px(10.)),
                                );
                                window.paint_quad(fill(grip, frame_color).corner_radii(px(1.5)));
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            );

        let label_width = if with_date {
            76.
        } else if with_seconds {
            54.
        } else {
            42.
        };
        let edge = |text: String, right: bool| {
            div()
                .flex_shrink_0()
                .w(px(label_width))
                .when(right, |el| el.text_right())
                .text_size(px(11.))
                .text_color(muted)
                .whitespace_nowrap()
                .child(text)
        };
        let status = if following {
            let refreshing = self.interval_ms.is_some();
            let offline = self.stored.as_ref().is_some_and(|stored| matches!(&stored.status, client::history::SyncStatus::Failed { reason, .. } if reason == "离线查看"));
            let state_color = if refreshing && !offline && has_data {
                theme.success
            } else {
                muted
            };
            h_flex()
                .justify_center()
                .gap_1p5()
                .text_size(px(11.))
                .text_color(state_color)
                .child(div().size(px(5.)).rounded_full().bg(state_color))
                .child(if offline {
                    "离线"
                } else if refreshing {
                    "实时"
                } else {
                    "已暂停"
                })
                .into_any_element()
        } else {
            Button::new("trend-live")
                .small()
                .ghost()
                .size(px(26.))
                .rounded_full()
                .accessibility_label("回到实时")
                .child(
                    Icon::from(IconName::ArrowRight)
                        .size(px(15.))
                        .text_color(theme.primary),
                )
                .tooltip("回到实时")
                .on_click(cx.listener(Self::resume_live))
                .into_any_element()
        };

        h_flex()
            .w_full()
            .min_w_0()
            .h(px(32.))
            .flex_shrink_0()
            .gap_2()
            .px_2p5()
            .rounded_full()
            .border_1()
            .border_color(border.opacity(0.48))
            .child(edge(label(extent.0), false))
            .child(strip)
            .child(edge(label(extent.1), true))
            .child(
                h_flex()
                    .w(px(52.))
                    .h_full()
                    .flex_shrink_0()
                    .justify_center()
                    .child(status),
            )
            .into_any_element()
    }

    /// 鼠标移出滑块时仍然响应拖动，释放左键后取消捕获。
    pub(super) fn navigator_drag_capture(&self, cx: &mut Context<Self>) -> AnyElement {
        let plot = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = plot.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moving.update(cx, |this, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.navigator_brush_move(event.position.x);
                            } else {
                                this.navigator_brush_up(event.position.x);
                            }
                            cx.notify();
                        });
                    }
                });
                let ending = plot;
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = ending.update(cx, |this, cx| {
                            this.navigator_brush_up(event.position.x);
                            cx.notify();
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0()
        .into_any_element()
    }

    fn brush_at(&self, x: Pixels) -> Option<Handle> {
        let bounds = self.nav_bounds.get()?;
        let width = f32::from(bounds.size.width);
        let frame = brush_frame(self.view.extent(), self.view.shown(), width);
        hit_handle(frame, f32::from(x - bounds.left()))
    }

    fn navigator_brush_down(&mut self, x: Pixels) {
        let Some(bounds) = self.nav_bounds.get() else {
            return;
        };
        let extent = self.view.extent();
        let Some(at) = time_at(bounds, extent, x) else {
            return;
        };
        let (mut start, mut end) = self.view.shown();
        let handle = self.brush_at(x);
        if handle.is_none() {
            let span = (end - start).clamp(self.view.narrowest(), extent.1 - extent.0);
            start = (at - span / 2.).clamp(extent.0, extent.1 - span);
            end = start + span;
            self.view.set_window(start, end, SLOW);
        }
        let handle = handle.unwrap_or(Handle::Window);
        self.nav_cursor = if handle == Handle::Window {
            CursorStyle::ClosedHand
        } else {
            handle.cursor()
        };
        self.drag = Some(Brush {
            handle,
            extent,
            window: (start, end),
            offset: at - if handle == Handle::End { end } else { start },
        });
        self.hover = None;
    }

    fn navigator_brush_move(&mut self, x: Pixels) {
        let (Some(drag), Some(bounds)) = (self.drag, self.nav_bounds.get()) else {
            return;
        };
        let Some(at) = time_at(bounds, drag.extent, x) else {
            return;
        };
        drag.update_view(&mut self.view, at);
    }

    fn navigator_brush_up(&mut self, x: Pixels) {
        self.drag = None;
        self.nav_cursor = self
            .brush_at(x)
            .map_or(CursorStyle::PointingHand, Handle::cursor);
    }
}

fn brush_frame(extent: (f64, f64), window: (f64, f64), width: f32) -> (f32, f32) {
    let length = (extent.1 - extent.0).max(1.);
    let x_of = |at: f64| ((at - extent.0) / length).clamp(0., 1.) as f32 * width;
    let (left, right) = (x_of(window.0), x_of(window.1));
    let size = (right - left).max(MIN_FRAME_PX.min(width));
    let left = ((left + right - size) * 0.5).clamp(0., width - size);
    (left, left + size)
}

fn hit_handle((left, right): (f32, f32), x: f32) -> Option<Handle> {
    let inside = (left..=right).contains(&x);
    let edge = if inside {
        EDGE_PX.min((right - left) / 3.)
    } else {
        EDGE_PX
    };
    let (start_gap, end_gap) = ((x - left).abs(), (x - right).abs());
    if start_gap <= edge && start_gap <= end_gap {
        Some(Handle::Start)
    } else if end_gap <= edge {
        Some(Handle::End)
    } else {
        inside.then_some(Handle::Window)
    }
}

fn time_at(bounds: Bounds<Pixels>, extent: (f64, f64), x: Pixels) -> Option<f64> {
    (bounds.size.width > px(0.)).then(|| {
        extent.0 + f64::from((x - bounds.left()) / bounds.size.width) * (extent.1 - extent.0)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Start,
    End,
    Window,
}

impl Handle {
    fn cursor(self) -> CursorStyle {
        match self {
            Self::Start | Self::End => CursorStyle::ResizeLeftRight,
            Self::Window => CursorStyle::OpenHand,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Brush {
    handle: Handle,
    extent: (f64, f64),
    window: (f64, f64),
    offset: f64,
}

impl Brush {
    fn update_view(self, view: &mut Viewport, at: f64) {
        let edge = at - self.offset;
        if self.handle == Handle::Start && view.following() {
            // 左手柄按拖动时的坐标换算，窗口右端继续追踪最新采样。
            view.set_span(view.live_edge() - edge, Duration::ZERO);
            return;
        }

        let (first, last) = self.extent;
        let (start, end) = self.window;
        let minimum = view.narrowest().min(last - first);
        let (a, b) = match self.handle {
            Handle::Start => (edge.clamp(first, (end - minimum).max(first)), end),
            Handle::End => (start, edge.clamp((start + minimum).min(last), last)),
            Handle::Window => {
                let span = end - start;
                let from = edge.clamp(first, (last - span).max(first));
                (from, from + span)
            }
        };
        view.set_window(a, b, Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::{Brush, Handle, Viewport, brush_frame, hit_handle};

    #[test]
    fn minimum_width_still_has_two_handles_and_a_draggable_middle() {
        let frame = brush_frame((0., 10_000.), (9_990., 10_000.), 400.);
        assert_eq!(frame, (390., 400.));
        assert_eq!(hit_handle(frame, 391.), Some(Handle::Start));
        assert_eq!(hit_handle(frame, 395.), Some(Handle::Window));
        assert_eq!(hit_handle(frame, 399.), Some(Handle::End));
        assert_eq!(hit_handle(frame, 300.), None);
    }

    #[test]
    fn handles_keep_the_opposite_edge_and_only_the_live_start_keeps_following() {
        let mut view = Viewport::default();
        view.extend(0., 600_000.);
        let mut brush = Brush {
            handle: Handle::Start,
            extent: view.extent(),
            window: view.shown(),
            offset: 500.,
        };
        view.extend(0., 620_000.);
        brush.update_view(&mut view, 400_500.);
        assert_eq!(view.shown(), (400_000., 620_000.));
        assert!(view.following());

        brush.handle = Handle::End;
        brush.extent = view.extent();
        brush.window = view.shown();
        view.extend(0., 640_000.);
        brush.update_view(&mut view, 550_500.);
        assert_eq!(view.shown(), (400_000., 550_000.));
        assert!(!view.following());

        brush.handle = Handle::Start;
        brush.window = view.shown();
        brush.update_view(&mut view, 550_500.);
        assert_eq!(view.shown(), (540_000., 550_000.));
    }
}
