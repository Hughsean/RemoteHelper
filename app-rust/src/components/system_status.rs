use common::SystemInfo;
use dioxus::prelude::*;

#[component]
pub fn SystemStatusDisplay(status: SystemInfo) -> Element {
    rsx! {
        div { class: "status-grid",
            StatusCard {
                title: "CPU 使用率",
                value: format!("{:.1}%", status.cpu_usage),
                subtext: status.cpu_model.clone(),
                percent: status.cpu_usage,
                color: "status-bar-red",
            }
            StatusCard {
                title: "内存使用率",
                value: if status.total_memory > 0 { format!("{:.1}%", (status.memory_usage as f32 / status.total_memory as f32) * 100.0) } else { "0.0%".to_string() },
                subtext: format!(
                    "{}/{} GB",
                    status.memory_usage / 1024 / 1024 / 1024,
                    status.total_memory / 1024 / 1024 / 1024,
                ),
                percent: if status.total_memory > 0 { (status.memory_usage as f32 / status.total_memory as f32) * 100.0 } else { 0.0 },
                color: "status-bar-blue",
            }
            if let Some(gpu_usage) = status.gpu_usage {
                GpuStatusCard {
                    usage: gpu_usage,
                    memory_usage: status.gpu_memory_usage,
                    total_memory: status.gpu_total_memory,
                    model: status.gpu_model.clone().unwrap_or_default(),
                }
            }
        
        }
    }
}

#[component]
fn StatusCard(title: String, value: String, subtext: String, percent: f32, color: String) -> Element {
    rsx! {
        div { class: "status-card",
            div { class: "status-header",
                div {
                    h3 { class: "status-title", "{title}" }
                    div { class: "status-value", "{value}" }
                }
            }
            div { class: "progress-track",
                div {
                    class: "progress-fill {color}",
                    style: "width: {percent}%",
                }
            }
            div { class: "status-subtext", "{subtext}" }
        }
    }
}

#[component]
fn GpuStatusCard(
    usage: u32,
    memory_usage: Option<u64>,
    total_memory: Option<u64>,
    model: String
) -> Element {
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
        format!("{:.1}/{:.1} GB", used as f64 / 1024.0 / 1024.0 / 1024.0, total as f64 / 1024.0 / 1024.0 / 1024.0)
    } else {
        "N/A".to_string()
    };

    rsx! {
        div { class: "status-card",
            div { class: "status-header",
                div {
                    h3 { class: "status-title", "GPU 状态" }
                    div { class: "status-value", "{usage}%" }
                }
            }

            div { class: "mb-2",
                div { class: "flex justify-between text-xs text-gray-400 mb-1",
                    span { "利用率" }
                    span { "{usage}%" }
                }
                div { class: "progress-track",
                    div {
                        class: "progress-fill status-bar-purple",
                        style: "width: {usage}%",
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

            div { class: "status-subtext mt-2", "{model}" }
        }
    }
}
