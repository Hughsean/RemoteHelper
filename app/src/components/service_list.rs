use common::ServiceInfo;
use dioxus::prelude::*;

#[component]
pub fn ServiceList(
    services: Vec<ServiceInfo>,
    on_control: EventHandler<(usize, String)>,
    on_add: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "service-list-container",
            div { class: "service-list-header",
                h2 { class: "service-list-title", "服务列表" }
                button {
                    class: "btn-add-service",
                    onclick: move |_| on_add.call(()),
                    "添加服务"
                }
            }
            div { class: "service-list-body",
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
    let status_class = if service.running {
        "status-running"
    } else {
        "status-stopped"
    };
    let status_text = if service.running {
        "运行中"
    } else {
        "已停止"
    };
    let icon_class = if service.running {
        "icon-running"
    } else {
        "icon-stopped"
    };

    rsx! {
        div { class: "service-item",
            div { class: "service-info",
                div { class: "service-icon-box {icon_class}",
                    // Icon placeholder
                    div { class: "service-dot {status_class}" }
                }
                div {
                    h3 { class: "service-name", "{service.description}" }
                    div { class: "service-meta",
                        span { class: "service-status {status_class}", "{status_text}" }
                        if let Some(pid) = service.pid {
                            span { class: "meta-separator", "•" }
                            span { class: "meta-pid", "PID: {pid}" }
                        }
                    }
                }
            }
            div { class: "service-actions",
                if service.running {
                    button {
                        class: "btn-icon btn-restart",
                        title: "重启",
                        onclick: move |_| on_control.call((service.id, "restart".to_string())),
                        "重启"
                    }
                    button {
                        class: "btn-icon btn-stop",
                        title: "停止",
                        onclick: move |_| on_control.call((service.id, "stop".to_string())),
                        "停止"
                    }
                } else {
                    button {
                        class: "btn-icon btn-start",
                        title: "启动",
                        onclick: move |_| on_control.call((service.id, "start".to_string())),
                        "启动"
                    }
                }
            }
        }
    }
}
