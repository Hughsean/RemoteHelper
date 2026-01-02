//! 主页面 - Dashboard
//!
//! 整合所有组件，展示完整的监控界面

use crate::backend;
use crate::components::{
    AddServiceModal, Header, Login, ServiceList, SystemStatusDisplay, TrendChart,
};
use crate::state::{use_auth_state, use_services_state, use_system_state};
use dioxus::prelude::*;
use std::time::Duration;

/// 多功能弹窗类型
#[derive(Clone, PartialEq)]
pub enum ModalType {
    None,
    Error(String),
    Form,
}

#[component]
pub fn Home() -> Element {
    let auth = use_auth_state();
    let mut system = use_system_state();
    let mut services = use_services_state();

    // 多功能弹窗状态
    let mut modal_type = use_signal(|| ModalType::None);
    let mut form_input = use_signal(String::new);

    // 如果未认证，显示登录界面
    if !auth.read().authenticated {
        return rsx! {
            Login {}
        };
    }

    // 自动刷新数据
    use_resource(move || {
        async move {
            loop {
                let interval = system.read().refresh_interval;

                // 如果间隔为0，暂停刷新
                if interval == 0 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }

                // 获取系统状态
                if let Ok(status) = backend::get_status(Some(interval)).await {
                    system.write().update_status(status);
                }

                // 获取服务列表
                if let Ok(services_list) = backend::list_services().await {
                    services.write().update_services(services_list);
                }

                tokio::time::sleep(Duration::from_millis(interval)).await;
            }
        }
    });

    rsx! {
        div { class: "app-container",
            Header {}

            div { class: "main-content",
                // 系统状态卡片
                if let Some(status) = &system.read().current_status {
                    SystemStatusDisplay { status: status.clone() }
                } else {
                    div { class: "loading-state", "正在加载系统状态..." }
                }

                // 趋势图表
                TrendChart { history: system.read().history.clone() }

                // 服务列表
                ServiceList {}
            }

            // 添加服务模态框
            if services.read().show_add_modal {
                AddServiceModal {}
            }

            // 凭据输入模态框 - 使用插槽组件
            {
                // 在 rsx 块内读取以确保响应式订阅
                let show_modal = services.read().show_credential_modal;
                tracing::info!("检查凭据弹窗状态: show_modal={}", show_modal);

                if show_modal {
                    tracing::info!("渲染凭据弹窗");
                    let services_data = services.read();
                    let pending_action = services_data.pending_credential_action.clone();
                    let username = services_data.credential_username.clone();
                    let password = services_data.credential_password.clone();
                    let pending_service = pending_action
                        .as_ref()
                        .and_then(|(id, _)| services_data.find_service(*id));

                    let action = pending_action
                        .as_ref()
                        // 头部

                        // 内容

                        // 底部
                        .map(|(_, a)| a.clone())
                        .unwrap_or_default();
                    let service_name = pending_service
                        .as_ref()
                        .map(|s| s.description.clone())
                        .unwrap_or_default();
                    let service_id = pending_service.as_ref().map(|s| s.id).unwrap_or(0);
                    drop(services_data);
                    let handle_credential_submit = {
                        let username = username.clone();
                        let password = password.clone();
                        let action = action.clone();
                        move |_| {
                            let user_name = username.clone();
                            let pwd = password.clone();
                            let action_str = action.clone();
                            if user_name.trim().is_empty() || pwd.is_empty() {
                                tracing::error!("用户名和密码不能为空");
                                return;
                            }
                            services.write().hide_credential_modal();
                            services.write().set_operating(service_id);
                            spawn(async move {
                                let result = backend::control_service(
                                        service_id,
                                        action_str,
                                        Some(user_name),
                                        Some(pwd),
                                    )
                                    .await;
                                if let Err(e) = result {
                                    tracing::error!("控制服务失败: {}", e);
                                }
                                if let Ok(new_services) = backend::list_services().await {
                                    services.write().update_services(new_services);
                                }
                                services.write().clear_operating();
                            });
                        }
                    };
                    let is_disabled = username.trim().is_empty() || password.is_empty();
                    rsx! {
                        div {
                            class: "modal-overlay",
                            onclick: move |_| services.write().hide_credential_modal(),
                            div { class: "modal", onclick: move |e| e.stop_propagation(),
                                div { class: "modal-header",
                                    h3 { class: "modal-title", "用户模式服务需要凭据" }
                                    button {
                                        class: "btn-close",
                                        onclick: move |_| services.write().hide_credential_modal(),
                                        "×"
                                    }
                                }

                                div { class: "modal-body",
                                    p {
                                        class: "credential-hint",
                                        style: "color: var(--text-slate-400); font-size: 0.875rem; margin-bottom: 1rem; line-height: 1.5;",
                                        "服务「{service_name}」需要以用户模式运行，请输入 Windows 用户凭据："
                                    }

                                    div { class: "form-group",
                                        label { class: "form-label", "用户名" }
                                        input {
                                            r#type: "text",
                                            class: "form-input",
                                            placeholder: "例如: Administrator 或 .\\用户名",
                                            value: "{username}",
                                            oninput: move |e| services.write().set_credential_username(e.value()),
                                        }
                                    }

                                    div { class: "form-group",
                                        label { class: "form-label", "密码" }
                                        input {
                                            r#type: "password",
                                            class: "form-input",
                                            placeholder: "Windows 用户密码",
                                            value: "{password}",
                                            oninput: move |e| services.write().set_credential_password(e.value()),
                                        }
                                    }
                                }

                                div { class: "modal-footer",
                                    button {
                                        class: "btn btn-secondary",
                                        onclick: move |_| services.write().hide_credential_modal(),
                                        "取消"
                                    }
                                    button {
                                        class: "btn btn-primary",
                                        onclick: handle_credential_submit,
                                        disabled: is_disabled,
                                        if action == "start" {
                                            "启动服务"
                                        } else {
                                            "重启服务"
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    rsx! {}
                }
            }

            // 多功能弹窗示例 - 根据类型显示不同内容
            match modal_type() {
                ModalType::Error(ref msg) => rsx! {
                    div { class: "modal-overlay", onclick: move |_| modal_type.set(ModalType::None),
                        div { class: "modal", onclick: move |e| e.stop_propagation(),
                            // 头部
                            div { class: "modal-header",
                                h3 { class: "modal-title", "错误提示" }
                                button {
                                    class: "btn-close",
                                    onclick: move |_| modal_type.set(ModalType::None),
                                    "×"
                                }
                            }

                    // 内容


                            // 底部
                            // 头部

                            // 内容
                            div { class: "modal-body",

                                // 底部
                                div { class: "error-message",
                                    p { "{msg}" }
                                }
                            }

                            div { class: "modal-footer",
                                button {
                                    class: "btn btn-primary",
                                    onclick: move |_| modal_type.set(ModalType::None),
                                    "确定"
                                }
                            }
                        }
                    }
                },
                ModalType::Form => rsx! {
                    div { class: "modal-overlay", onclick: move |_| modal_type.set(ModalType::None),
                        div { class: "modal", onclick: move |e| e.stop_propagation(),
                            div { class: "modal-header",
                                h3 { class: "modal-title", "填写表单" }
                                button {
                                    class: "btn-close",
                                    onclick: move |_| modal_type.set(ModalType::None),
                                    "×"
                                }
                            }

                            div { class: "modal-body",
                                div { class: "form-group",
                                    label { class: "form-label", "请输入内容" }
                                    input {
                                        r#type: "text",
                                        class: "form-input",
                                        value: "{form_input}",
                                        oninput: move |e| form_input.set(e.value()),
                                        placeholder: "在这里输入...",
                                    }
                                }
                            }

                            div { class: "modal-footer",
                                button {
                                    class: "btn btn-secondary",
                                    onclick: move |_| modal_type.set(ModalType::None),
                                    "取消"
                                }
                                button {
                                    class: "btn btn-primary",
                                    onclick: move |_| {
                                        tracing::info!("表单提交: {}", form_input());
                                        modal_type.set(ModalType::None);
                                    },
                                    "提交"
                                }
                            }
                        }
                    }
                },
                ModalType::None => rsx! {},
            }

            // 测试按钮 - 可以删除
            div { style: "position: fixed; bottom: 20px; right: 20px; display: flex; gap: 10px; z-index: 100;",
                button {
                    class: "btn btn-primary",
                    onclick: move |_| {
                        modal_type.set(ModalType::Error("这是一个错误消息示例！".to_string()))
                    },
                    "显示错误"
                }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| modal_type.set(ModalType::Form),
                    "显示表单"
                }
            }
        }
    }
}
