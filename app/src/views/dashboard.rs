use dioxus::prelude::*;
use crate::components::{MetricCard, MetricCardData, MetricItem, TrendChart, TrendChartData};

const DASHBOARD_CSS: Asset = asset!("/assets/styling/dashboard.css");

#[component]
pub fn Dashboard() -> Element {
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
        ],
        footer: Some("正在获取硬件信息...".to_string()),
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
    });

    // 趋势图数据
    let mut cpu_trend = use_signal(|| TrendChartData {
        title: "CPU 使用率趋势".to_string(),
        current_value: "0.0".to_string(),
        unit: "%".to_string(),
        data_points: vec![],
        max_points: 60,
        color: "#ef4444".to_string(),
    });

    let mut memory_trend = use_signal(|| TrendChartData {
        title: "内存使用率趋势".to_string(),
        current_value: "0.0".to_string(),
        unit: "%".to_string(),
        data_points: vec![],
        max_points: 60,
        color: "#3b82f6".to_string(),
    });

    let mut gpu_trend = use_signal(|| TrendChartData {
        title: "GPU 使用率趋势".to_string(),
        current_value: "0".to_string(),
        unit: "%".to_string(),
        data_points: vec![],
        max_points: 60,
        color: "#10b981".to_string(),
    });

    // 实时数据更新
    use_future(move || async move {
        loop {
            match client::send_request(common::Request::GetStatus { interval_ms: None }).await {
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
                    };

                    // 更新 CPU 趋势图
                    cpu_trend.write().add_point(cpu_usage, timestamp);
                    cpu_trend.write().current_value = format!("{:.1}", cpu_usage);

                    // 更新内存趋势图
                    memory_trend.write().add_point(mem_usage_percent, timestamp);
                    memory_trend.write().current_value = format!("{:.1}", mem_usage_percent);

                    // 更新 GPU 数据
                    if let (Some(gpu_usage), Some(gpu_mem_used), Some(gpu_mem_total), Some(gpu_model)) =
                        (info.gpu_usage, info.gpu_memory_usage, info.gpu_total_memory, info.gpu_model)
                    {
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
                                    value: format!("{:.1}/{:.1} GB", gpu_mem_used_gb, gpu_mem_total_gb),
                                    progress: Some(gpu_mem_ratio),
                                },
                            ],
                            footer: Some(gpu_model),
                        };

                        // 更新 GPU 趋势图
                        gpu_trend.write().add_point(gpu_usage as f32, timestamp);
                        gpu_trend.write().current_value = format!("{}", gpu_usage);
                    } else {
                        *gpu_data.write() = MetricCardData {
                            title: "GPU 状态".to_string(),
                            main_value: "不可用".to_string(),
                            items: vec![],
                            footer: Some("未检测到 GPU 或驱动未安装".to_string()),
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

            // 每秒更新一次
            #[cfg(feature = "web")]
            gloo_timers::future::TimeoutFuture::new(1000).await;

            #[cfg(feature = "desktop")]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: DASHBOARD_CSS }

        div { class: "dashboard-container",
            h1 { class: "dashboard-title", "系统监控仪表板" }

            div { class: "metrics-grid",
                MetricCard { data: cpu_data }
                MetricCard { data: gpu_data }
            }

            h2 { class: "section-title", "历史趋势" }

            div { class: "trends-grid",
                TrendChart { data: cpu_trend }
                TrendChart { data: memory_trend }
                TrendChart { data: gpu_trend }
            }

            div { class: "dashboard-footer",
                p { "💡 实时数据每秒自动更新" }
            }
        }
    }
}

