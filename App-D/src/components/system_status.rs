//! 系统状态显示组件

use common::SystemInfo;
use dioxus::prelude::*;

#[component]
pub fn SystemStatusDisplay(status: SystemInfo) -> Element {
    rsx! {
        div { class: "status-grid",
            CpuCard {
                cpu_usage: status.cpu_usage,
                cpu_model: status.cpu_model.clone(),
                memory_usage: status.memory_usage,
                total_memory: status.total_memory,
            }
            GpuStatusCard {
                usage: status.gpu_usage,
                memory_usage: status.gpu_memory_usage,
                total_memory: status.gpu_total_memory,
                model: status.gpu_model.clone().unwrap_or_else(|| "N/A".to_string()),
            }
        }
    }
}

#[component]
fn CpuCard(cpu_usage: f32, cpu_model: String, memory_usage: u64, total_memory: u64) -> Element {
    let mem_percent = if total_memory > 0 {
        (memory_usage as f32 / total_memory as f32) * 100.0
    } else {
        0.0
    };
    let used_gb = memory_usage as f64 / 1024.0 / 1024.0 / 1024.0;
    let total_gb = total_memory as f64 / 1024.0 / 1024.0 / 1024.0;

    rsx! {
        div { class: "status-card",
            div { class: "status-header",
                div {
                    h3 { class: "status-title", "CPU & 内存" }
                    div { class: "status-value", "{cpu_usage:.1}%" }
                }
            }

            div { class: "mb-2",
                div { class: "flex justify-between text-xs text-gray-400 mb-1",
                    span { "CPU 利用率" }
                    span { "{cpu_usage:.1}%" }
                }
                div { class: "progress-track",
                    div {
                        class: "progress-fill status-bar-red",
                        style: "width: {cpu_usage}%",
                    }
                }
            }

            div {
                div { class: "flex justify-between text-xs text-gray-400 mb-1",
                    span { "内存使用" }
                    span { "{used_gb:.1}/{total_gb:.1} GB" }
                }
                div { class: "progress-track",
                    div {
                        class: "progress-fill status-bar-blue",
                        style: "width: {mem_percent}%",
                    }
                }
            }

            div { class: "status-subtext mt-auto pt-2", "{cpu_model}" }
        }
    }
}

#[component]
fn GpuStatusCard(
    usage: Option<u32>,
    memory_usage: Option<u64>,
    total_memory: Option<u64>,
    model: String,
) -> Element {
    let usage_value = usage.unwrap_or(0);
    let usage_text = if usage.is_some() {
        format!("{usage_value}%")
    } else {
        "N/A".to_string()
    };

    let mem_percent = if let (Some(used), Some(total)) = (memory_usage, total_memory) {
        if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let mem_text = if let (Some(used), Some(total)) = (memory_usage, total_memory) {
        format!(
            "{:.1}/{:.1} GB",
            used as f64 / 1024.0 / 1024.0 / 1024.0,
            total as f64 / 1024.0 / 1024.0 / 1024.0
        )
    } else {
        "N/A".to_string()
    };

    rsx! {
        div { class: "status-card",
            div { class: "status-header",
                div {
                    h3 { class: "status-title", "GPU 状态" }
                    div { class: "status-value", "{usage_text}" }
                }
            }

            div { class: "mb-2",
                div { class: "flex justify-between text-xs text-gray-400 mb-1",
                    span { "利用率" }
                    span { "{usage_text}" }
                }
                div { class: "progress-track",
                    div {
                        class: "progress-fill status-bar-emerald",
                        style: "width: {usage_value}%",
                    }
                }
            }

            div {
                div { class: "flex justify-between text-xs text-gray-400 mb-1",
                    span { "显存" }
                    span { "{mem_text}" }
                }
                div { class: "progress-track",
                    div {
                        class: "progress-fill status-bar-blue",
                        style: "width: {mem_percent}%",
                    }
                }
            }

            div { class: "status-subtext mt-auto", "{model}" }
        }
    }
}
