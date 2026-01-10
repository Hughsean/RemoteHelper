use crate::components::AddServiceDialog;
use common::{Request, Response, ServiceAction};
use dioxus::prelude::*;

const SERVICES_CSS: Asset = asset!("/assets/styling/services.css");

#[component]
pub fn Services() -> Element {
    // 服务列表状态
    let services = use_signal(|| Vec::new());
    let mut show_add_dialog = use_signal(|| false);

    // 加载服务列表的函数
    let load_services = move || {
        let mut services_clone = services.clone();
        spawn(async move {
            match client::send_request(Request::ListServices).await {
                Ok(Response::Services(service_list)) => {
                    services_clone.set(service_list);
                }
                Ok(Response::Error(e)) => {
                    tracing::error!("获取服务列表失败: {}", e);
                    services_clone.set(vec![]);
                }
                _ => {
                    tracing::error!("获取服务列表失败: 意外响应");
                    services_clone.set(vec![]);
                }
            }
        });
    };

    // 控制服务函数
    let control_service = move |id: usize, action: ServiceAction| {
        let load_services_clone = load_services.clone();
        spawn(async move {
            let result = client::send_request(Request::ControlService {
                id,
                action: action.clone(),
                user_name: None,
                user_password: None,
            })
            .await;

            match result {
                Ok(Response::Ok) => {
                    tracing::info!("服务控制成功: {} {:?}", id, action);
                    // 重新获取服务列表
                    load_services_clone();
                }
                Ok(Response::Error(e)) => {
                    tracing::error!("服务控制失败: {}", e);
                }
                _ => {
                    tracing::error!("服务控制失败: 意外响应");
                }
            }
        });
    };

    // 组件挂载时加载服务列表
    use_effect(move || {
        load_services();
    });

    rsx! {
        document::Link { rel: "stylesheet", href: SERVICES_CSS }
        div { class: "services-page",
            // 页面头部
            div { class: "services-header",
                h1 { "服务列表" }
                button {
                    class: "refresh-button",
                    onclick: move |_| show_add_dialog.set(true),
                    "添加服务"
                }
            }

            // 服务列表
            div { class: "services-list",
                if services().is_empty() {
                    div { class: "loading-state",
                        div { class: "loading-spinner" }
                        "正在加载服务列表..."
                    }
                } else {
                    for service in services() {
                        div { class: "service-item", key: "{service.id}",
                            div { class: "service-icon",
                                div { class: if service.running { "icon-circle running" } else { "icon-circle stopped" },
                                    "⚙"
                                }
                            }
                            div { class: "service-info",
                                div { class: "service-title",
                                    if service.run_as_user {
                                        span { class: "icon", "⚙️" }
                                    }
                                    "{service.description}"
                                }
                                div { class: "service-meta",
                                    span { class: if service.running { "status-text running" } else { "status-text stopped" },
                                        if service.running {
                                            "运行中"
                                        } else {
                                            "已停止"
                                        }
                                    }
                                    span { class: "separator", "•" }
                                    span { class: "service-type", "系统服务" }
                                    if let Some(pid) = service.pid {
                                        span { class: "separator", "•" }
                                        span { class: "service-pid", "PID: {pid}" }
                                    }
                                }
                            }
                            div { class: "service-controls",
                                if service.running {
                                    button {
                                        onclick: move |_| control_service(service.id, ServiceAction::Restart),
                                        class: "btn-action",
                                        "重启"
                                    }
                                    button {
                                        onclick: move |_| control_service(service.id, ServiceAction::Stop),
                                        class: "btn-action",
                                        "停止"
                                    }
                                } else {
                                    button {
                                        onclick: move |_| control_service(service.id, ServiceAction::Start),
                                        class: "btn-action btn-start",
                                        "启动"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 添加服务对话框
            if show_add_dialog() {
                AddServiceDialog {
                    on_close: move |_| show_add_dialog.set(false),
                    on_success: move |_| {
                        show_add_dialog.set(false);
                        load_services();
                    },
                }
            }
        }
    }
}
