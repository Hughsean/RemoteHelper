//! 服务列表组件

use crate::backend;
use crate::state::use_services_state;
use common::ServiceInfo;
use dioxus::prelude::*;

#[component]
pub fn ServiceList() -> Element {
    let mut services = use_services_state();

    let services_data = services.read();

    rsx! {
        div { class: "service-list-container",
            div { class: "service-list-header",
                h2 { class: "service-list-title", "服务列表" }
                button {
                    class: "btn-add-service",
                    onclick: move |_| {
                        services.write().show_add_modal();
                    },
                    "添加服务"
                }
            }
            div { class: "service-list-body",
                for service in services_data.services.iter() {
                    ServiceItem { key: "{service.id}", service: service.clone() }
                }
            }
        }
    }
}

#[component]
fn ServiceItem(service: ServiceInfo) -> Element {
    let mut services = use_services_state();

    let is_operating = services.read().is_operating(service.id);

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

    let mut handle_control = move |action: &str| {
        let service_id = service.id;
        let action = action.to_string();

        services.write().set_operating(service_id);

        spawn(async move {
            let result = backend::control_service(service_id, action).await;

            if let Err(e) = result {
                tracing::error!("控制服务失败: {}", e);
            }

            // 刷新服务列表
            if let Ok(new_services) = backend::list_services().await {
                services.write().update_services(new_services);
            }

            services.write().clear_operating();
        });
    };

    rsx! {
        div { class: "service-item",
            div { class: "service-info",
                div { class: "service-icon-box {icon_class}",
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
                        disabled: is_operating,
                        onclick: move |_| handle_control("restart"),
                        if is_operating {
                            "操作中..."
                        } else {
                            "重启"
                        }
                    }
                    button {
                        class: "btn-icon btn-stop",
                        title: "停止",
                        disabled: is_operating,
                        onclick: move |_| handle_control("stop"),
                        if is_operating {
                            "操作中..."
                        } else {
                            "停止"
                        }
                    }
                } else {
                    button {
                        class: "btn-icon btn-start",
                        title: "启动",
                        disabled: is_operating,
                        onclick: move |_| handle_control("start"),
                        if is_operating {
                            "操作中..."
                        } else {
                            "启动"
                        }
                    }
                }
            }
        }
    }
}
