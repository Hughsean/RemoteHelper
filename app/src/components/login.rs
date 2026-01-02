//! 登录组件
//!
//! 使用新的状态管理模式，直接通过 use_auth_state 访问认证状态

use crate::backend;
use crate::state::use_auth_state;
use dioxus::prelude::*;

#[component]
pub fn Login() -> Element {
    let mut auth = use_auth_state();
    let nav = use_navigator();

    let mut passphrase = use_signal(String::new);
    let mut loading = use_signal(|| false);

    let handle_submit = move |evt: FormEvent| {
        evt.stop_propagation();
        evt.prevent_default();

        if loading() {
            return;
        }

        let pass = passphrase();
        let addr = auth.read().server_addr.clone();

        loading.set(true);

        spawn(async move {
            match backend::authenticate(pass, addr).await {
                Ok(_) => {
                    auth.write().set_authenticated();
                    // 登录成功后导航到 Dashboard
                    nav.push("/dashboard");
                }
                Err(e) => {
                    auth.write().set_auth_failed(e);
                }
            }
            loading.set(false);
        });
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "login-card",
                div { class: "login-header",
                    div { class: "login-icon",
                        svg {
                            class: "icon-lg",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z",
                            }
                        }
                    }
                    h2 { class: "login-title", "需要身份验证" }
                    p { class: "login-subtitle", "请输入凭据以继续" }
                }

                form { onsubmit: handle_submit, class: "login-form",
                    div { class: "form-group",
                        label { class: "form-label", "服务器地址" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{auth.read().server_addr}",
                            oninput: move |e| {
                                auth.write().server_addr = e.value();
                            },
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "密钥文件密码" }
                        input {
                            r#type: "password",
                            class: "form-input",
                            placeholder: "~/id_ed25519.json 的密码",
                            value: "{passphrase}",
                            oninput: move |e| passphrase.set(e.value()),
                            disabled: loading(),
                        }
                    }

                    if let Some(err) = &auth.read().error_msg {
                        div { class: "error-message", "{err}" }
                    }

                    button {
                        r#type: "submit",
                        class: "btn-primary w-full",
                        disabled: loading(),
                        if loading() {
                            "连接中..."
                        } else {
                            "连接"
                        }
                    }
                }
            }
        }
    }
}
