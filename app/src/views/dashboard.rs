use crate::theme::workspace_background;
use crate::ui::{message, panel, request_error, small_button};
use crate::views::trend::{SERIES, TrendPlot, series_color};
use common::{Request, Response, SystemInfo};
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::Button,
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use std::time::Duration;
use tokio::runtime::Handle;

const INTERVALS: [(Option<u64>, &str); 4] = [
    (Some(500), "0.5 秒"),
    (Some(1000), "1 秒"),
    (Some(3000), "3 秒"),
    (None, "暂停"),
];
const RANGES: [(u64, &str); 4] = [
    (60_000, "1 分钟"),
    (300_000, "5 分钟"),
    (1_800_000, "30 分钟"),
    (10_800_000, "3 小时"),
];
pub(crate) struct Dashboard {
    latest: Option<SystemInfo>,
    trend: Entity<TrendPlot>,
    interval: Option<u64>,
    range: u64,
    visible: [bool; 3],
    error: Option<String>,
    _polling: Task<()>,
}

impl Dashboard {
    pub(crate) fn new(runtime: Handle, cx: &mut Context<Self>) -> Self {
        let polling = cx.spawn(async move |this, cx| {
            while let Ok(interval) = this.update(cx, |view, _| view.interval) {
                if let Some(interval_ms) = interval {
                    let task = runtime.spawn(client::send_request(Request::GetStatus {
                        interval_ms: Some(interval_ms),
                    }));
                    let result = match task.await {
                        Ok(Ok(Response::Status(info))) => Ok(info),
                        Ok(Ok(response)) => Err(request_error(response)),
                        Ok(Err(error)) => Err(error),
                        Err(error) => Err(error.to_string()),
                    };
                    if this
                        .update(cx, |view, cx| {
                            match result {
                                Ok(info) if view.interval.is_some() => view.record(info, cx),
                                Ok(_) => {}
                                Err(error) => view.error = Some(error),
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(interval.unwrap_or(500)))
                    .await;
            }
        });
        Self {
            latest: None,
            trend: cx.new(|_| TrendPlot::new(RANGES[0].0, RANGES[3].0)),
            interval: Some(1000),
            range: RANGES[0].0,
            visible: [true; 3],
            error: None,
            _polling: polling,
        }
    }

    fn record(&mut self, info: SystemInfo, cx: &mut Context<Self>) {
        let values = [
            Some(info.cpu_usage),
            usage(info.memory_usage, info.total_memory),
            info.gpu_usage.map(|value| value as f32),
        ];
        self.trend
            .update(cx, |plot, cx| plot.record(info.timestamp, values, cx));
        self.latest = Some(info);
        self.error = None;
    }

    fn set_range(&mut self, range: u64, cx: &mut Context<Self>) {
        self.range = range;
        self.trend.update(cx, |plot, cx| plot.set_range(range, cx));
        cx.notify();
    }

    fn set_interval(&mut self, interval: Option<u64>, cx: &mut Context<Self>) {
        self.interval = interval;
        self.trend
            .update(cx, |plot, cx| plot.set_interval(interval, cx));
        cx.notify();
    }

    fn option_menu<T: Copy + PartialEq + 'static>(
        id: &'static str,
        selected: T,
        options: &'static [(T, &'static str)],
        update: fn(&mut Self, T, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let label = options
            .iter()
            .find(|(value, _)| *value == selected)
            .map_or("—", |(_, label)| *label);
        let view = cx.weak_entity();
        Button::new(id)
            .small()
            .outline()
            .child(
                h_flex()
                    .gap_2()
                    .child(label)
                    .child(Icon::from(IconName::ChevronDown).small()),
            )
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                options.iter().fold(menu, |menu, &(value, label)| {
                    let view = view.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(value == selected)
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |view, cx| update(view, value, cx));
                            }),
                    )
                })
            })
    }

    fn render_chart(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;

        panel(cx)
            .flex_grow_1()
            .flex_shrink_0()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .flex_shrink_0()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child("资源使用率趋势"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_xs().text_color(muted).child("时间范围"))
                            .child(Self::option_menu(
                                "range",
                                self.range,
                                &RANGES,
                                Self::set_range,
                                cx,
                            )),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .children(SERIES.iter().enumerate().map(|(index, (label, _))| {
                        let color = series_color(index);
                        h_flex()
                            .gap_1()
                            .pl_2()
                            .child(div().size_2().rounded_full().bg(if self.visible[index] {
                                color
                            } else {
                                muted
                            }))
                            .child(small_button(("series", index), *label, false, cx).on_click(
                                cx.listener(move |view, _, _, cx| {
                                    view.visible[index] = !view.visible[index];
                                    view.trend.update(cx, |plot, cx| {
                                        plot.set_visible(index, view.visible[index], cx)
                                    });
                                    cx.notify();
                                }),
                            ))
                    })),
            )
            .child(
                v_flex()
                    .flex_grow_1()
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_stretch()
                            .flex_grow_1()
                            .flex_shrink_0()
                            .min_h(px(176.))
                            .gap_2()
                            .child(
                                div()
                                    .relative()
                                    .w(px(34.))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .children(
                                        ["100%", "75%", "50%", "25%", "0%"]
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, label)| {
                                                div()
                                                    .absolute()
                                                    .right_0()
                                                    .top(relative(index as f32 / 4.))
                                                    .mt(px(-7.))
                                                    .line_height(px(14.))
                                                    .child(label)
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .min_w_0()
                                    .flex_1()
                                    .bg(workspace_background())
                                    .child(div().absolute().inset_0().child(self.trend.clone())),
                            ),
                    )
                    .child(
                        h_flex()
                            .pl(px(42.))
                            .flex_shrink_0()
                            .justify_between()
                            .text_xs()
                            .text_color(muted)
                            .child(format!(
                                "{}前",
                                RANGES
                                    .iter()
                                    .find(|(range, _)| *range == self.range)
                                    .map_or("", |(_, label)| *label)
                            ))
                            .child("现在"),
                    ),
            )
    }
}

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (status, status_color) = if self.interval.is_none() {
            ("已暂停", muted)
        } else if self.error.is_some() {
            ("刷新失败", cx.theme().danger)
        } else if self.latest.is_none() {
            ("正在获取数据", muted)
        } else {
            ("实时更新", cx.theme().success)
        };

        v_flex()
            .id("dashboard-scroll")
            .size_full()
            .overflow_y_scroll()
            .gap_3()
            .p_3()
            .child(
                h_flex()
                    .justify_between()
                    .flex_shrink_0()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("系统监控"))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(status_color)
                                    .child(div().size_2().rounded_full().bg(status_color))
                                    .child(status),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_xs().text_color(muted).child("刷新间隔"))
                            .child(Self::option_menu(
                                "interval",
                                self.interval,
                                &INTERVALS,
                                Self::set_interval,
                                cx,
                            )),
                    ),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|error| message(error.clone(), true, cx)),
            )
            .child(
                panel(cx)
                    .flex_shrink_0()
                    .flex_row()
                    .items_stretch()
                    .gap_0()
                    .p_0()
                    .child(cpu_card(self.latest.as_ref(), cx).flex_1())
                    .child(
                        memory_card(self.latest.as_ref(), cx)
                            .flex_1()
                            .border_l_1()
                            .border_color(cx.theme().border),
                    )
                    .child(
                        gpu_card(self.latest.as_ref(), cx)
                            .flex_1()
                            .border_l_1()
                            .border_color(cx.theme().border),
                    ),
            )
            .child(self.render_chart(cx))
    }
}

fn cpu_card(info: Option<&SystemInfo>, cx: &App) -> Div {
    let model = info
        .map(|info| info.cpu_model.as_str())
        .filter(|name| !name.is_empty());
    metric_card(0, IconName::Cpu, info.map(|info| info.cpu_usage), model, cx)
        .child(metric_row(
            "温度",
            info.and_then(|info| info.cpu_temperature)
                .map(|value| format!("{value:.1} °C")),
            cx,
        ))
        .child(metric_row(
            "封装功率",
            info.and_then(|info| info.cpu_package_power)
                .map(|value| format!("{value:.1} W")),
            cx,
        ))
}

fn memory_card(info: Option<&SystemInfo>, cx: &App) -> Div {
    let percent = info.and_then(|info| usage(info.memory_usage, info.total_memory));
    let used = info.map(|info| gibibytes(info.memory_usage));
    let total = info.map(|info| gibibytes(info.total_memory));
    metric_card(1, IconName::MemoryStick, percent, Some("物理内存"), cx)
        .child(metric_row("已使用", used, cx))
        .child(metric_row("总容量", total, cx))
}

fn gpu_card(info: Option<&SystemInfo>, cx: &App) -> Div {
    let percent = info
        .and_then(|info| info.gpu_usage)
        .map(|usage| usage as f32);
    let memory = info.and_then(|info| {
        Some(format!(
            "{:.1} / {}",
            gib_value(info.gpu_memory_usage?),
            gibibytes(info.gpu_total_memory?)
        ))
    });
    let model = info.and_then(|info| info.gpu_model.as_deref());
    let temperature = info
        .and_then(|info| info.gpu_temperature)
        .map(|value| format!("{value:.1} °C"));
    let power = info
        .and_then(|info| info.gpu_power_watts)
        .map(|value| format!("{value:.1} W"));
    let sensors = match (temperature, power) {
        (Some(temperature), Some(power)) => Some(format!("{temperature} · {power}")),
        (temperature, power) => temperature.or(power),
    };
    metric_card(2, IconName::Monitor, percent, model, cx)
        .child(metric_row("显存", memory, cx))
        .child(metric_row("温度 / 功率", sensors, cx))
}

fn metric_card(
    index: usize,
    icon: IconName,
    percent: Option<f32>,
    subtitle: Option<&str>,
    cx: &App,
) -> Div {
    let color = series_color(index);
    let theme = cx.theme();
    let percent = percent.filter(|value| value.is_finite());
    v_flex()
        .min_w_0()
        .p_3()
        .gap_1p5()
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.))
                        .text_color(color)
                        .child(Icon::from(icon).small()),
                )
                .child(div().text_sm().font_semibold().child(SERIES[index].0)),
        )
        .child(
            div()
                .text_size(px(23.))
                .font_semibold()
                .child(percent.map_or_else(|| "—".into(), |value| format!("{value:.1}%"))),
        )
        .child(
            div()
                .h_1()
                .w_full()
                .rounded_full()
                .bg(color.opacity(0.12))
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(color)
                        .w(relative(percent.unwrap_or(0.).clamp(0., 100.) / 100.)),
                ),
        )
        .child(
            div()
                .truncate()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(subtitle.unwrap_or("设备信息不可用").to_string()),
        )
}

fn metric_row(label: &'static str, value: Option<String>, cx: &App) -> Div {
    h_flex()
        .justify_between()
        .gap_1()
        .text_xs()
        .child(div().text_color(cx.theme().muted_foreground).child(label))
        .child(
            div()
                .text_color(cx.theme().foreground)
                .font_medium()
                .truncate()
                .child(value.unwrap_or_else(|| "—".to_string())),
        )
}

fn gib_value(bytes: u64) -> f64 {
    bytes as f64 / (1024. * 1024. * 1024.)
}

fn gibibytes(bytes: u64) -> String {
    format!("{:.1} GiB", gib_value(bytes))
}

fn usage(used: u64, total: u64) -> Option<f32> {
    (total > 0).then(|| used as f32 / total as f32 * 100.)
}
