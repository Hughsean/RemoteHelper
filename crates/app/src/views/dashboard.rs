use crate::notifications::Notices;
use crate::ui::{ConnectionStatus, panel, request_error, trend_title};
use crate::views::trend::{Playback, SERIES, TrendPlot, series_colors};
use client::history::{HistoryCache, HistoryQuery, HistorySync, SyncStatus};
use common::{Request, Response, SystemInfo, history_api::TimeRange};
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tokio::runtime::Handle;

const INTERVALS: [(Option<u64>, &str); 4] = [
    (Some(500), "0.5 秒"),
    (Some(1000), "1 秒"),
    (Some(3000), "3 秒"),
    (None, "暂停"),
];
const INTERVAL_FONT_SIZE: f32 = 13.;

pub(crate) struct Dashboard {
    latest: Option<SystemInfo>,
    trend: Entity<TrendPlot>,
    interval: Option<u64>,
    visible: [bool; 3],
    refresh_failed: bool,
    poll_active: bool,
    history_task: Option<Task<()>>,
    _polling: Task<()>,
}

impl Dashboard {
    pub(crate) fn new(
        runtime: Handle,
        notices: Notices,
        connection_status: Entity<ConnectionStatus>,
        cx: &mut Context<Self>,
    ) -> Self {
        let polling = cx.spawn(async move |this, cx| {
            while let Ok(interval) =
                this.update(cx, |view, _| view.interval.filter(|_| view.poll_active))
            {
                if let Some(interval_ms) = interval {
                    let task = runtime.spawn(client::send_request(Request::GetStatus {
                        interval_ms: Some(interval_ms),
                    }));
                    let response = task.await;
                    let status = if matches!(response, Ok(Ok(_))) {
                        ConnectionStatus::Online
                    } else {
                        ConnectionStatus::Offline
                    };
                    let result = match response {
                        Ok(Ok(Response::Status(info))) => Ok(info),
                        Ok(Ok(response)) => Err(request_error(response)),
                        Ok(Err(error)) => Err(error),
                        Err(error) => Err(error.to_string()),
                    };
                    if this
                        .update(cx, |view, cx| {
                            if !view.poll_active {
                                return;
                            }
                            connection_status
                                .update(cx, |connection, cx| connection.set(status, cx));
                            match result {
                                Ok(info) if view.interval.is_some() => view.record(info, cx),
                                Ok(_) => {}
                                Err(error) if view.interval.is_some() && !view.refresh_failed => {
                                    view.refresh_failed = true;
                                    notices.error(format!("无法刷新系统监控：{error}"), cx);
                                }
                                Err(_) => {}
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
            trend: cx.new(TrendPlot::new),
            interval: Some(1000),
            visible: [true; 3],
            refresh_failed: false,
            poll_active: true,
            history_task: None,
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
            .update(cx, |plot, cx| plot.record_live(&info, values, cx));
        self.latest = Some(info);
        self.refresh_failed = false;
    }

    fn set_interval(&mut self, interval: Option<u64>, cx: &mut Context<Self>) {
        if self.interval.is_none() {
            self.refresh_failed = false;
        }
        self.interval = interval;
        self.trend
            .update(cx, |plot, cx| plot.set_interval(interval, cx));
        cx.notify();
    }

    pub(crate) fn set_polling(&mut self, active: bool, cx: &mut Context<Self>) {
        self.poll_active = active;
        if !active {
            self.clear_chart_hover(cx);
        }
    }

    pub(crate) fn history_sources(
        &mut self,
        runtime: Handle,
        sync: Option<Arc<HistorySync>>,
        cache: Option<HistoryCache>,
        anchor: Option<i64>,
        window: Duration,
        refresh: Duration,
        clear: bool,
        cx: &mut Context<Self>,
    ) {
        self.history_task = None;
        let window_ms = window.as_millis().clamp(1, i64::MAX as u128) as i64;
        self.trend.update(cx, |plot, cx| {
            plot.set_history_window(window_ms as u64, cx);
            if clear {
                plot.set_history(HistoryQuery::default(), cx);
            }
        });
        if sync.is_none() && cache.is_none() {
            return;
        }
        let offline = sync.is_none();
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let mut read_cache = cache;
            let mut query_band = None;
            let mut query_pending = true;
            let mut online_error = None;
            let mut active: Option<AbortWork<Result<HistoryCache, client::history::HistoryError>>> =
                None;
            let mut deadline = Instant::now();
            while let Ok(viewport) = this.update(cx, |view, cx| view.trend.read(cx).history_range())
            {
                let fallback = || {
                    let to = anchor.unwrap_or_else(wall_ms).saturating_add(1);
                    TimeRange {
                        from: to.saturating_sub(window_ms),
                        to,
                    }
                };
                let visible = viewport.unwrap_or_else(fallback);
                if query_band
                    .is_none_or(|band: TimeRange| visible.from < band.from || visible.to > band.to)
                {
                    let pad = (visible.to.saturating_sub(visible.from) / 8)
                        .clamp(30_000, 6 * 60 * 60_000);
                    query_band = Some(TimeRange {
                        from: visible.from.saturating_sub(pad),
                        to: visible.to.saturating_add(pad),
                    });
                    active = None;
                    query_pending = true;
                    deadline = Instant::now() + Duration::from_millis(150);
                }
                let band = query_band.unwrap_or(visible);
                if active.as_ref().is_some_and(|work| work.0.is_finished()) {
                    let mut completed = active.take().expect("已完成的历史同步任务");
                    match (&mut completed.0).await {
                        Ok(Ok(cache)) => {
                            read_cache = Some(cache);
                            online_error = None;
                        }
                        Ok(Err(err)) => {
                            online_error = Some(err.to_string());
                        }
                        Err(err) => {
                            online_error = Some(err.to_string());
                        }
                    }
                    query_pending = true;
                    deadline = Instant::now()
                        + if online_error.is_some() {
                            refresh.min(Duration::from_secs(10))
                        } else {
                            refresh
                        };
                }
                if query_pending {
                    if let Some(cache) = read_cache
                        .clone()
                        .or_else(|| sync.as_ref().and_then(|sync| sync.active_cache()))
                    {
                        let mut work =
                            AbortWork(runtime.spawn(async move { cache.query(band).await }));
                        if let Ok(Ok(mut query)) = (&mut work.0).await {
                            if offline || online_error.is_some() {
                                query.status = SyncStatus::Failed {
                                    at_ms: wall_ms(),
                                    reason: online_error
                                        .clone()
                                        .unwrap_or_else(|| "离线查看".into()),
                                };
                            }
                            let mut prepare = AbortWork(
                                runtime.spawn_blocking(move || Playback::new(query, Some(band))),
                            );
                            let Ok(prepared) = (&mut prepare.0).await else {
                                break;
                            };
                            let applied = this.update(cx, |view, cx| {
                                let visible =
                                    view.trend.read(cx).history_range().unwrap_or(visible);
                                if visible.from < band.from || visible.to > band.to {
                                    return false;
                                }
                                view.trend
                                    .update(cx, |plot, cx| plot.set_prepared_history(prepared, cx));
                                true
                            });
                            match applied {
                                Ok(true) => query_pending = false,
                                Ok(false) => {}
                                Err(_) => return,
                            }
                        } else {
                            query_pending = false;
                        }
                    } else {
                        query_pending = false;
                    }
                }
                if active.is_none()
                    && Instant::now() >= deadline
                    && let Some(sync) = sync.clone()
                {
                    active = Some(AbortWork(
                        runtime.spawn(async move { sync.refresh(Some(band)).await }),
                    ));
                }
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            }
        }));
    }

    pub(crate) fn clear_chart_hover(&mut self, cx: &mut Context<Self>) {
        self.trend.update(cx, TrendPlot::clear_hover);
    }

    pub(crate) fn interval_control(dashboard: &Entity<Self>, cx: &App) -> Div {
        let selected = dashboard.read(cx).interval;
        let label = INTERVALS
            .iter()
            .find(|(value, _)| *value == selected)
            .map_or("—", |(_, label)| *label);
        let view = dashboard.downgrade();
        let menu = Button::new("interval")
            .xsmall()
            .compact()
            .ghost()
            .h(px(20.))
            .px(px(5.))
            .accessibility_label(format!("刷新间隔 {label}"))
            .child(
                h_flex()
                    .gap_1()
                    .text_size(px(INTERVAL_FONT_SIZE))
                    .child(label)
                    .child(Icon::from(IconName::ChevronDown).size(px(11.))),
            )
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                INTERVALS.into_iter().fold(menu, |menu, (value, label)| {
                    let view = view.clone();
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(value == selected)
                            .on_click(move |_, _, cx| {
                                let _ = view.update(cx, |view, cx| view.set_interval(value, cx));
                            }),
                    )
                })
            });
        h_flex()
            .flex_shrink_0()
            .gap_1p5()
            .child(
                div()
                    .text_size(px(INTERVAL_FONT_SIZE))
                    .text_color(cx.theme().muted_foreground)
                    .child("刷新间隔"),
            )
            .child(menu)
    }

    fn render_chart(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let colors = series_colors(cx);

        panel(cx)
            .flex_grow_1()
            .flex_shrink_0()
            .gap_3()
            .p_4()
            .border_color(cx.theme().border.opacity(0.62))
            .child(
                h_flex()
                    .justify_between()
                    .flex_shrink_0()
                    .gap_3()
                    .child(trend_title())
                    .child(h_flex().flex_shrink_0().gap_1().children(
                        SERIES.iter().enumerate().map(|(index, label)| {
                            let color = colors[index];
                            let active = self.visible[index];
                            let indicator = if active { color } else { muted.opacity(0.6) };
                            Button::new(("series", index))
                                .small()
                                .accessibility_label(*label)
                                .toggled(active)
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(color.opacity(if active { 0.07 } else { 0. }))
                                        .foreground(if active {
                                            cx.theme().foreground
                                        } else {
                                            muted
                                        })
                                        .hover(color.opacity(0.18))
                                        .active(color.opacity(0.25)),
                                )
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .child(
                                            div().w(px(9.)).h(px(2.)).rounded_full().bg(indicator),
                                        )
                                        .child(*label),
                                )
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.visible[index] = !view.visible[index];
                                    view.trend.update(cx, |plot, cx| {
                                        plot.set_visible(index, view.visible[index], cx)
                                    });
                                    cx.notify();
                                }))
                        }),
                    )),
            )
            .child(
                v_flex()
                    .flex_grow_1()
                    .flex_shrink_0()
                    .min_h(px(240.))
                    .child(self.trend.clone()),
            )
    }
}

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("dashboard-scroll")
            .size_full()
            .overflow_y_scroll()
            .gap_3()
            .p_3()
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

struct AbortWork<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortWork<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or_default()
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
    let color = series_colors(cx)[index];
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
                .child(div().text_sm().font_semibold().child(SERIES[index])),
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
