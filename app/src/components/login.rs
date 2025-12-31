use dioxus::prelude::*;

#[component]
pub fn Login(on_login: EventHandler<(String, String)>) -> Element {
    let mut passphrase = use_signal(|| String::new());
    let mut address = use_signal(|| "frp-try.com:53460".to_string());
    let error = use_signal(|| Option::<String>::None);

    let handle_submit = move |evt: FormEvent| {
        evt.stop_propagation();
        evt.prevent_default();
        on_login.call((passphrase(), address()));
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
                            value: "{address}",
                            oninput: move |e| address.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "访问密钥" }
                        input {
                            r#type: "password",
                            class: "form-input",
                            value: "{passphrase}",
                            oninput: move |e| passphrase.set(e.value()),
                        }
                    }
                    if let Some(err) = error() {
                        div { class: "error-message", "{err}" }
                    }
                    button { r#type: "submit", class: "btn-primary w-full", "连接" }
                }
            }
        }
    }
}
