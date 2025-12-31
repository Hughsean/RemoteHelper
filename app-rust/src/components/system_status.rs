use crate::models::SystemInfo;
use dioxus::prelude::*;

#[component]
pub fn SystemStatusDisplay(status: SystemInfo) -> Element {
    rsx! {
        div { class: "grid grid-cols-1 md:grid-cols-3 gap-6 mb-8",
            StatusCard {
                title: "CPU Usage",
                value: format!("{:.1}%", status.cpu_usage),
                subtext: status.cpu_model.clone(),
                percent: status.cpu_usage,
                color: "bg-red-500",
            }
            StatusCard {
                title: "Memory Usage",
                value: format!("{:.1}%", (status.memory_usage as f32 / status.total_memory as f32) * 100.0),
                subtext: format!(
                    "{}/{} GB",
                    status.memory_usage / 1024 / 1024 / 1024,
                    status.total_memory / 1024 / 1024 / 1024,
                ),
                percent: (status.memory_usage as f32 / status.total_memory as f32) * 100.0,
                color: "bg-blue-500",
            }
            if let Some(gpu_usage) = status.gpu_usage {
                StatusCard {
                    title: "GPU Usage",
                    value: format!("{}%", gpu_usage),
                    subtext: status.gpu_model.clone().unwrap_or_default(),
                    percent: gpu_usage as f32,
                    color: "bg-purple-500",
                }
            }
        
        }
    }
}

#[component]
fn StatusCard(title: String, value: String, subtext: String, percent: f32, color: String) -> Element {
    rsx! {
        div { class: "bg-slate-900/50 backdrop-blur-sm rounded-xl p-5 border border-slate-800/50 shadow-lg hover:border-indigo-500/30 transition-all duration-300 group",
            div { class: "flex justify-between items-start mb-4",
                div {
                    h3 { class: "text-slate-400 text-sm font-medium uppercase tracking-wider",
                        "{title}"
                    }
                    div { class: "text-2xl font-bold text-white mt-1", "{value}" }
                }
            }
            div { class: "w-full bg-slate-800 rounded-full h-2 mb-2 overflow-hidden",
                div {
                    class: "h-full rounded-full transition-all duration-500 {color}",
                    style: "width: {percent}%",
                }
            }
            div { class: "text-xs text-slate-500 truncate", "{subtext}" }
        }
    }
}
