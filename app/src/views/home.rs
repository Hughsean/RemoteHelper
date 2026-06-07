use crate::views::Services;
use crate::widgets::{
    DataSeries, MetricCard, MetricCardData, MetricItem, TrendChart, TrendChartData,
};
use dioxus::prelude::*;
use dioxus_primitives::select::{Select, SelectList, SelectOption, SelectTrigger, SelectValue};

const HOME_CSS: Asset = asset!("/assets/styling/home.css");
const SELECT_CSS: Asset = asset!("/assets/components/select/style.css");

/// 页面视图选项
#[derive(Clone, PartialEq, Debug)]
enum PageView {
    Home,
    Services,
}

// impl PageView {
//     fn to_display(&self) -> &'static str {
//         match self {
//             PageView::Home => "主页",
//             PageView::Services => "服务管理",
//             PageView::Test => "测试页面",
//         }
//     }
// }

/// 更新间隔选项
#[derive(Clone, PartialEq, Debug)]
enum UpdateInterval {
    HalfSecond,
    OneSecond,
    ThreeSeconds,
    Paused,
}

impl UpdateInterval {
    fn to_millis(&self) -> Option<u64> {
        match self {
            UpdateInterval::HalfSecond => Some(500),
            UpdateInterval::OneSecond => Some(1000),
            UpdateInterval::ThreeSeconds => Some(3000),
            UpdateInterval::Paused => None,
        }
    }

    fn to_display(&self) -> &'static str {
        match self {
            UpdateInterval::HalfSecond => "0.5秒",
            UpdateInterval::OneSecond => "1秒",
            UpdateInterval::ThreeSeconds => "3秒",
            UpdateInterval::Paused => "暂停",
        }
    }
}

#[component]
pub fn Home() -> Element {
    // 当前页面视图状态
    let mut current_view = use_signal(|| PageView::Home);

    // 更新间隔状态
    let mut update_interval = use_signal(|| UpdateInterval::OneSecond);

    // 使用 Signal 存储实时数据
    let mut cpu_data = use_signal(|| MetricCardData {
        title: "CPU & 内存".to_string(),
        main_value: "加载中...".to_string(),
        items: vec![
            MetricItem {
                label: "CPU 利用率".to_string(),
                value: "-- %".to_string(),
                progress: Some(0.0),
            },
            MetricItem {
                label: "内存使用".to_string(),
                value: "-- / -- GB".to_string(),
                progress: Some(0.0),
            },
            MetricItem {
                label: "CPU 温度".to_string(),
                value: "-- °C".to_string(),
                progress: None,
            },
            MetricItem {
                label: "CPU 封装功率".to_string(),
                value: "-- W".to_string(),
                progress: None,
            },
        ],
        footer: Some("正在获取硬件信息...".to_string()),
        right_small_power: None,
        right_small_temp: None,
    });

    let mut gpu_data = use_signal(|| MetricCardData {
        title: "GPU 状态".to_string(),
        main_value: "加载中...".to_string(),
        items: vec![
            MetricItem {
                label: "利用率".to_string(),
                value: "-- %".to_string(),
                progress: Some(0.0),
            },
            MetricItem {
                label: "显存".to_string(),
                value: "-- / -- GB".to_string(),
                progress: Some(0.0),
            },
        ],
        footer: Some("正在获取硬件信息...".to_string()),
        right_small_power: None,
        right_small_temp: None,
    });

    // 综合趋势图数据 - 包含 CPU、内存、GPU 三个系列
    let mut system_trend = use_signal(|| {
        let mut chart_data = TrendChartData::new("系统资源使用率趋势".to_string(), "%".to_string());

        chart_data.add_series(DataSeries::new("CPU".to_string(), "#ef4444".to_string()));
        chart_data.add_series(DataSeries::new("内存".to_string(), "#3b82f6".to_string()));
        chart_data.add_series(DataSeries::new("GPU".to_string(), "#10b981".to_string()));

        chart_data
    });

    // 实时数据更新
    use_future(move || async move {
        loop {
            // 检查是否暂停
            let current_interval = update_interval();
            if let Some(interval_ms) = current_interval.to_millis() {
                match client::send_request(common::Request::GetStatus {
                    interval_ms: Some(interval_ms),
                })
                .await
                {
                    Ok(common::Response::Status(info)) => {
                        let timestamp = info.timestamp;

                        // 更新 CPU 数据
                        let cpu_usage = info.cpu_usage;
                        let mem_used_gb = info.memory_usage as f64 / 1024.0 / 1024.0 / 1024.0;
                        let mem_total_gb = info.total_memory as f64 / 1024.0 / 1024.0 / 1024.0;
                        let mem_usage_ratio = info.memory_usage as f32 / info.total_memory as f32;
                        let mem_usage_percent = mem_usage_ratio * 100.0;

                        *cpu_data.write() = MetricCardData {
                            title: "CPU & 内存".to_string(),
                            main_value: format!("{:.1}%", cpu_usage),
                            items: vec![
                                MetricItem {
                                    label: "CPU 利用率".to_string(),
                                    value: format!("{:.1}%", cpu_usage),
                                    progress: Some(cpu_usage / 100.0),
                                },
                                MetricItem {
                                    label: "内存使用".to_string(),
                                    value: format!("{:.1}/{:.1} GB", mem_used_gb, mem_total_gb),
                                    progress: Some(mem_usage_ratio),
                                },
                            ],
                            footer: Some(info.cpu_model.clone()),
                            right_small_power: info
                                .cpu_package_power
                                .map(|p| (format!("{:.1} W", p), "#f59e0b".to_string())),
                            right_small_temp: info
                                .cpu_temperature
                                .map(|t| (format!("{:.1} °C", t), "#ef4444".to_string())),
                        };

                        // 更新系统趋势图 - CPU (系列 0)
                        system_trend
                            .write()
                            .add_point_to_series(0, cpu_usage, timestamp);
                        system_trend.write().series[0].current_value = format!("{:.1}", cpu_usage);

                        // 更新系统趋势图 - 内存 (系列 1)
                        system_trend
                            .write()
                            .add_point_to_series(1, mem_usage_percent, timestamp);
                        system_trend.write().series[1].current_value =
                            format!("{:.1}", mem_usage_percent);

                        // 更新 GPU 数据
                        if let (
                            Some(gpu_usage),
                            Some(gpu_mem_used),
                            Some(gpu_mem_total),
                            Some(gpu_model),
                        ) = (
                            info.gpu_usage,
                            info.gpu_memory_usage,
                            info.gpu_total_memory,
                            info.gpu_model,
                        ) {
                            let gpu_mem_used_gb = gpu_mem_used as f64 / 1024.0 / 1024.0 / 1024.0;
                            let gpu_mem_total_gb = gpu_mem_total as f64 / 1024.0 / 1024.0 / 1024.0;
                            let gpu_mem_ratio = gpu_mem_used as f32 / gpu_mem_total as f32;

                            *gpu_data.write() = MetricCardData {
                                title: "GPU 状态".to_string(),
                                main_value: format!("{}%", gpu_usage),
                                items: vec![
                                    MetricItem {
                                        label: "利用率".to_string(),
                                        value: format!("{}%", gpu_usage),
                                        progress: Some(gpu_usage as f32 / 100.0),
                                    },
                                    MetricItem {
                                        label: "显存".to_string(),
                                        value: format!(
                                            "{:.1}/{:.1} GB",
                                            gpu_mem_used_gb, gpu_mem_total_gb
                                        ),
                                        progress: Some(gpu_mem_ratio),
                                    },
                                ],
                                footer: Some(gpu_model),
                                right_small_power: info
                                    .gpu_power_watts
                                    .map(|p| (format!("{:.1} W", p), "#f59e0b".to_string())),
                                right_small_temp: info
                                    .gpu_temperature
                                    .map(|t| (format!("{:.1} °C", t), "#ef4444".to_string())),
                            };

                            // 更新系统趋势图 - GPU (系列 2)
                            system_trend.write().add_point_to_series(
                                2,
                                gpu_usage as f32,
                                timestamp,
                            );
                            system_trend.write().series[2].current_value = format!("{}", gpu_usage);
                        } else {
                            *gpu_data.write() = MetricCardData {
                                title: "GPU 状态".to_string(),
                                main_value: "不可用".to_string(),
                                items: vec![],
                                footer: Some("未检测到 GPU 或驱动未安装".to_string()),
                                right_small_power: None,
                                right_small_temp: None,
                            };
                        }
                    }
                    Err(e) => {
                        tracing::error!("获取系统状态失败: {}", e);
                    }
                    _ => {
                        tracing::warn!("收到意外的响应类型");
                    }
                }

                // 根据选择的间隔休眠
                #[cfg(feature = "web")]
                gloo_timers::future::TimeoutFuture::new(interval_ms as u32).await;

                #[cfg(feature = "desktop")]
                tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
            } else {
                // 暂停状态，等待一秒后重新检查
                #[cfg(feature = "web")]
                gloo_timers::future::TimeoutFuture::new(1000).await;

                #[cfg(feature = "desktop")]
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: HOME_CSS }
        document::Link { rel: "stylesheet", href: SELECT_CSS }

        div { class: "home-container",
            // Header bar with update interval control
            div { class: "home-header",
                div { class: "title-section",
                    h1 { class: "home-title", "系统监控" }
                    p { class: "powered-by", "Powered by Hughsean" }
                }

                div { class: "header-navigation",
                    button {
                        class: "nav-button",
                        class: if *current_view.read() == PageView::Home { "nav-button active" } else { "nav-button" },
                        onclick: move |_| {
                            current_view.set(PageView::Home);
                        },
                        "仪表板"
                    }
                    button {
                        class: "nav-button",
                        class: if *current_view.read() == PageView::Services { "nav-button active" } else { "nav-button" },
                        onclick: move |_| {
                            current_view.set(PageView::Services);
                        },
                        "服务管理"
                    }
                }

                div { class: "header-controls",
                    label { class: "update-interval-label", "更新间隔:" }
                    Select::<String> {
                        class: "select",
                        default_value: Some(update_interval().to_display().to_string()),
                        on_value_change: move |value: Option<String>| {
                            if let Some(v) = value {
                                let interval = match v.as_str() {
                                    "0.5秒" => UpdateInterval::HalfSecond,
                                    "1秒" => UpdateInterval::OneSecond,
                                    "3秒" => UpdateInterval::ThreeSeconds,
                                    "暂停" => UpdateInterval::Paused,
                                    _ => UpdateInterval::OneSecond,
                                };
                                update_interval.set(interval);
                            }
                        },
                        SelectTrigger { class: "select-trigger",
                            SelectValue {}
                            svg {
                                class: "select-expand-icon",
                                view_box: "0 0 24 24",
                                xmlns: "http://www.w3.org/2000/svg",
                                polyline { points: "6 9 12 15 18 9" }
                            }
                        }
                        SelectList { class: "select-list",
                            SelectOption::<String> {
                                class: "select-option",
                                index: use_signal(|| 0),
                                value: "0.5秒".to_string(),
                                "0.5秒"
                            }
                            SelectOption::<String> {
                                class: "select-option",
                                index: use_signal(|| 1),
                                value: "1秒".to_string(),
                                "1秒"
                            }
                            SelectOption::<String> {
                                class: "select-option",
                                index: use_signal(|| 2),
                                value: "3秒".to_string(),
                                "3秒"
                            }
                            SelectOption::<String> {
                                class: "select-option",
                                index: use_signal(|| 3),
                                value: "暂停".to_string(),
                                "暂停"
                            }
                        }
                    }
                }
            }

            // 根据当前视图显示不同内容
            match current_view() {
                PageView::Home => rsx! {
                    div { class: "metrics-grid",
                        MetricCard { data: cpu_data }
                        MetricCard { data: gpu_data }
                    }

                    // div { class: "home-footer",
                    //     p { "💡 实时数据自动更新 • 当前间隔: {update_interval().to_display()}" }
                    //     p { class: "powered-by", "Powered by Hughsean" }
                    // }

                    div { class: "trends-single",
                        TrendChart { data: system_trend }
                    }
                },
                PageView::Services => rsx! {
                    div { class: "content-section", Services {} }
                },
            }
        }
    }
}
