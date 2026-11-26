//! 开发用预览：不连服务器，灌一段合成数据渲染趋势图，方便离线核对界面。
//!
//! 用法：`$env:REMOTEHELPER_PREVIEW="1"; cargo run -p App`，窗口保持打开，
//! 由外部截图。只在 debug 构建里编译。

use crate::theme;
use crate::ui::trend_title;
use crate::views::trend::{SERIES, TrendPlot, series_colors};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Preview {
    trend: Entity<TrendPlot>,
    _polling: Task<()>,
}

impl Preview {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64);
        let trend = cx.new(TrendPlot::new);
        // 两小时、两秒一条的合成历史。
        trend.update(cx, |plot, cx| plot.seed(now, 2_000, 3_600, cx));

        let feeding = trend.clone();
        let polling = cx.spawn_in(window, async move |_, cx| {
            let mut index = 3_600usize;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(1_000))
                    .await;
                index += 1;
                let at = now + (index as u64 - 3_600) * 2_000;
                let phase = index as f32 / 41.;
                let values = [
                    Some(35.0 + 28.0 * phase.sin() + 4.0 * (phase * 7.).sin()),
                    Some(46.0 + 12.0 * (phase * 0.37).cos()),
                    Some(22.0 + 18.0 * (phase * 0.9).sin().abs()),
                ];
                feeding.update(cx, |plot, cx| plot.record(at, values, cx));
            }
        });

        // 两秒后放大到冻结态，核对进度条框和「实时」按钮。
        if std::env::var_os("REMOTEHELPER_PREVIEW_FROZEN").is_some() {
            let zooming = trend.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(2_000))
                    .await;
                zooming.update(cx, |plot, cx| plot.debug_zoom(0.12, cx));
            })
            .detach();
        }

        // 在图上放一个假指针，核对指针处的读数浮层。
        if std::env::var_os("REMOTEHELPER_PREVIEW_HOVER").is_some() {
            let hovering = trend.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(2_000))
                    .await;
                hovering.update(cx, |plot, cx| plot.debug_hover(0.42, 0.4, cx));
            })
            .detach();
        }

        Self {
            trend,
            _polling: polling,
        }
    }
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = series_colors(cx);
        v_flex()
            .size_full()
            .p_4()
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.62))
                    .bg(cx.theme().group_box)
                    .p_4()
                    .child(h_flex().justify_between().child(trend_title()).child(
                        h_flex().gap_3().children(SERIES.iter().enumerate().map(
                            |(index, name)| {
                                h_flex()
                                    .gap_1p5()
                                    .text_xs()
                                    .child(
                                        div().w(px(9.)).h(px(2.)).rounded_full().bg(colors[index]),
                                    )
                                    .child(*name)
                            },
                        )),
                    ))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(self.trend.clone()),
                    ),
            )
    }
}

/// 打开预览窗口。
pub(crate) fn open(cx: &mut App) -> Result<()> {
    gpui_kit::open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("RemoteHelper 预览".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1080.), px(760.)),
                cx,
            ))),
            window_background: theme::window_background(cx),
            ..Default::default()
        },
        cx,
        |window, cx| {
            theme::bind_window(window, cx);
            cx.new(|cx| Preview::new(window, cx))
        },
    )?;
    Ok(())
}
