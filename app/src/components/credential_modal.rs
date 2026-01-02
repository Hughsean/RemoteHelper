//! 凭据输入模态框组件
//!
//! 用于用户模式服务操作时输入凭据

use crate::backend;
use crate::state::use_services_state;
use dioxus::prelude::*;

#[component]
pub fn CredentialModal() -> Element {
    let mut services = use_services_state();

    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);

    // 提取提交处理逻辑到组件函数体中
    let mut handle_submit = move || {
        tracing::info!("确认按钮被点击");

        let username_val = username();
        let password_val = password();

        tracing::info!(
            "获取的用户名: {}, 密码长度: {}",
            username_val,
            password_val.len()
        );

        // 验证必填字段
        if username_val.trim().is_empty() || password_val.trim().is_empty() {
            tracing::warn!("验证失败: 用户名或密码不能为空");
            return;
        }

        // 获取待执行的操作
        let pending = services.read().pending_credential_action.clone();
        tracing::info!("待执行操作: {:?}", pending);

        if let Some((service_id, action)) = pending {
            tracing::info!(
                "提交凭据: service_id={}, action={}, username={}",
                service_id,
                action,
                username_val
            );

            // 标记服务为操作中
            services.write().set_operating(service_id);

            tracing::info!("准备发起异步请求");

            spawn(async move {
                tracing::info!("异步任务开始执行");
                tracing::info!(
                    "调用 control_service: id={}, action={}, user={}",
                    service_id,
                    action,
                    username_val
                );
                let result = backend::control_service(
                    service_id,
                    action.clone(),
                    Some(username_val),
                    Some(password_val),
                )
                .await;
                tracing::info!("control_service 返回结果: {:?}", result);
                if let Err(e) = result {
                    tracing::error!("控制服务失败: {}", e);
                }
                tracing::info!("开始刷新服务列表");
                if let Ok(new_services) = backend::list_services().await {
                    services.write().update_services(new_services);
                    tracing::info!("服务列表已更新");
                }
                services.write().clear_operating();

                // 在异步任务内部关闭模态框（模仿 AddServiceModal 的做法）
                services.write().hide_credential_modal();
                tracing::info!("操作完成");
            });
        } else {
            tracing::warn!("没有待执行的操作");
        }
    };

    let services_data = services.read();
    let action_text = if let Some((_, action)) = &services_data.pending_credential_action {
        match action.as_str() {
            "start" => "启动",
            "restart" => "重启",
            _ => "操作",
        }
    } else {
        "操作"
    };

    rsx! {
        div {
            class: "modal-overlay",
            onclick: move |_| services.write().hide_credential_modal(),
            div {
                class: "modal modal-credential",
                onclick: move |e| e.stop_propagation(),
                // 头部
                div { class: "modal-header",
                    h3 { class: "modal-title", "输入凭据" }
                    button {
                        class: "btn-close",
                        onclick: move |_| services.write().hide_credential_modal(),
                        "×"
                    }
                }

                // 内容
                div { class: "modal-body",
                    p { class: "credential-hint",
                        "此服务以用户模式运行，需要提供登录凭据才能{action_text}"
                    }
                    div { class: "modal-form",
                        div { class: "form-group",
                            label { class: "form-label", "用户名" }
                            input {
                                r#type: "text",
                                class: "form-input",
                                value: "{username}",
                                oninput: move |e| username.set(e.value()),
                                placeholder: "请输入用户名",
                                autofocus: true,
                            }
                        }

                        div { class: "form-group",
                            label { class: "form-label", "密码" }
                            input {
                                r#type: "password",
                                class: "form-input",
                                value: "{password}",
                                oninput: move |e| password.set(e.value()),
                                placeholder: "请输入密码",
                            }
                        }
                    }
                }

                // 底部按钮
                div { class: "modal-footer",
                    button {
                        class: "btn btn-secondary",
                        onclick: move |_| services.write().hide_credential_modal(),
                        "取消"
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| handle_submit(),
                        "确认"
                    }
                }
            }
        }
    }
}
