use common::ServiceInfo;
use dioxus::prelude::*;

#[component]
pub fn ServiceList(
    services: Vec<ServiceInfo>,
    on_control: EventHandler<(usize, String)>,
    on_add: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "bg-slate-900/50 backdrop-blur-sm rounded-xl border border-slate-800/50 shadow-lg overflow-hidden",
            div { class: "px-6 py-4 border-b border-slate-800/50 flex justify-between items-center bg-slate-900/30",
                h2 { class: "text-lg font-semibold text-white flex items-center gap-2",
                    "Services"
                }
                button {
                    class: "bg-indigo-600 hover:bg-indigo-500 text-white px-3 py-1.5 rounded-lg text-sm font-medium transition-colors flex items-center gap-2",
                    onclick: move |_| on_add.call(()),
                    "Add Service"
                }
            }
            div { class: "divide-y divide-slate-800/50",
                for service in services {
                    ServiceItem {
                        key: "{service.id}",
                        service: service.clone(),
                        on_control,
                    }
                }
            }
        }
    }
}

#[component]
fn ServiceItem(service: ServiceInfo, on_control: EventHandler<(usize, String)>) -> Element {
    let status_color = if service.running { "text-emerald-400" } else { "text-slate-500" };
    let status_text = if service.running { "Running" } else { "Stopped" };
    let bg_color = if service.running { "bg-emerald-500/10 border-emerald-500/20" } else { "bg-slate-800/50 border-slate-700" };

    rsx! {
        div { class: "p-4 hover:bg-slate-800/30 transition-colors flex items-center justify-between group",
            div { class: "flex items-center gap-4",
                div { class: "w-10 h-10 rounded-lg flex items-center justify-center {bg_color} border",
                    // Icon placeholder
                    div { class: "w-2 h-2 rounded-full {status_color.replace(\"text\", \"bg\")}" }
                }
                div {
                    h3 { class: "text-white font-medium", "{service.description}" }
                    div { class: "flex items-center gap-2 text-xs",
                        span { class: "{status_color} font-medium", "{status_text}" }
                        if let Some(pid) = service.pid {
                            span { class: "text-slate-600", "•" }
                            span { class: "text-slate-500 font-mono", "PID: {pid}" }
                        }
                    }
                }
            }
            div { class: "flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity",
                if service.running {
                    button {
                        class: "p-2 text-slate-400 hover:text-amber-400 hover:bg-amber-400/10 rounded-lg transition-all",
                        title: "Restart",
                        onclick: move |_| on_control.call((service.id, "restart".to_string())),
                        "Restart"
                    }
                    button {
                        class: "p-2 text-slate-400 hover:text-red-400 hover:bg-red-400/10 rounded-lg transition-all",
                        title: "Stop",
                        onclick: move |_| on_control.call((service.id, "stop".to_string())),
                        "Stop"
                    }
                } else {
                    button {
                        class: "p-2 text-slate-400 hover:text-emerald-400 hover:bg-emerald-400/10 rounded-lg transition-all",
                        title: "Start",
                        onclick: move |_| on_control.call((service.id, "start".to_string())),
                        "Start"
                    }
                }
            }
        }
    }
}
