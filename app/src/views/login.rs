use dioxus::prelude::*;

const LOGIN_CSS: Asset = asset!("/assets/styling/login.css");

#[component]
pub fn Login() -> Element {
    let mut server_address = use_signal(|| "frp-try.com:53460".to_string());
    let mut password = use_signal(|| String::new());
    let mut key_file_content = use_signal(|| Option::<String>::None);
    let mut key_file_name = use_signal(|| "未选择文件".to_string());
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut is_logging_in = use_signal(|| false);

    let navigator = use_navigator();

    // 在组件挂载时尝试读取用户home目录下的id_ed25519.json
    let _auto_load_key = use_resource(move || async move {
        #[cfg(feature = "desktop")]
        {
            if let Some(home_dir) = dirs::home_dir() {
                let key_path = home_dir.join("id_ed25519.json");
                if key_path.exists() {
                    match std::fs::read_to_string(&key_path) {
                        Ok(content) => {
                            key_file_content.set(Some(content));
                            key_file_name.set("id_ed25519.json (默认位置)".to_string());
                            return;
                        }
                        Err(_) => {
                            // 静默失败，用户可手动选择
                        }
                    }
                }
            }
        }
    });

    // 处理文件选择
    let on_file_change = move |evt: FormEvent| {
        spawn(async move {
            let files = evt.files();
            // let file_names = files.iter().map(|e| e.name()).collect::<Vec<_>>();
            if let Some(file) = files.first() {
                match file.read_string().await {
                    Ok(content) => {
                        key_file_content.set(Some(content));
                        key_file_name.set(file.name());
                        error_msg.set(None);
                    }
                    Err(e) => {
                        error_msg.set(Some(format!("读取文件失败: {}", e)));
                        key_file_content.set(None);
                        key_file_name.set("未选择文件".to_string());
                    }
                }
            }
        });
    };

    // 处理登录
    let handle_login = move || {
        spawn(async move {
            is_logging_in.set(true);
            error_msg.set(None);

            // 验证输入
            let key_content = match key_file_content() {
                Some(c) => c,
                None => {
                    error_msg.set(Some("请选择密钥文件".to_string()));
                    is_logging_in.set(false);
                    return;
                }
            };

            let pass = password();
            if pass.is_empty() {
                error_msg.set(Some("请输入密码".to_string()));
                is_logging_in.set(false);
                return;
            }

            // 解析密钥文件
            let key_file: common::func::KeyFile = match serde_json::from_str(&key_content) {
                Ok(kf) => kf,
                Err(e) => {
                    error_msg.set(Some(format!("密钥文件格式错误: {}", e)));
                    is_logging_in.set(false);
                    return;
                }
            };

            // 解密私钥
            let signing_key = match common::func::decrypt_private_key(&key_file, &pass) {
                Ok(key) => key,
                Err(e) => {
                    error_msg.set(Some(e));
                    is_logging_in.set(false);
                    return;
                }
            };

            // 设置全局状态
            {
                let mut guard = client::SIGNING_KEY.lock().unwrap();
                *guard = Some((signing_key, key_file.pub_key));
            }

            {
                let mut guard = client::SERVER_ADDRESS.lock().unwrap();
                *guard = server_address();
            }
            tracing::info!("尝试连接到服务器 {}", server_address());
            // 尝试连接
            match client::connect_and_auth().await {
                Ok(conn) => {
                    // 保存连接
                    let mut guard = client::CONNECTION.lock().await;
                    *guard = Some(conn);
                    drop(guard);

                    // 跳转到仪表板测试页面
                    navigator.push("/dashboard");
                }
                Err(e) => {
                    error_msg.set(Some(format!("连接失败: {}", e)));
                    is_logging_in.set(false);
                }
            }
        });
    };

    let on_login = move |_| {
        handle_login();
    };

    rsx! {
        document::Link { rel: "stylesheet", href: LOGIN_CSS }

        div { class: "login-container",
            div { class: "login-box",
                h1 { class: "login-title", "RemoteHelper 登录" }

                if let Some(err) = error_msg() {
                    div { class: "error-message", "{err}" }
                }

                div { class: "form-group",
                    label { class: "form-label", "服务器地址" }
                    input {
                        class: "input",
                        r#type: "text",
                        value: "{server_address}",
                        placeholder: "host:port",
                        disabled: is_logging_in(),
                        oninput: move |evt| server_address.set(evt.value()),
                    }
                }

                div { class: "form-group",
                    label { class: "form-label", "密钥文件" }
                    div { class: "file-input-wrapper",
                        input {
                            r#type: "file",
                            accept: ".json",
                            id: "key-file-input",
                            disabled: is_logging_in(),
                            onchange: on_file_change,
                        }
                        label {
                            r#for: "key-file-input",
                            class: "file-input-label",
                            "{key_file_name}"
                        }
                    }
                }

                div { class: "form-group",
                    label { class: "form-label", "密码" }
                    input {
                        class: "input",
                        r#type: "password",
                        value: "{password}",
                        placeholder: "密钥文件密码",
                        disabled: is_logging_in(),
                        oninput: move |evt| password.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter && !is_logging_in() {
                                handle_login();
                            }
                        },
                    }
                }

                div { class: "login-actions",
                    button {
                        class: "button",
                        "data-style": "primary",
                        disabled: is_logging_in(),
                        onclick: on_login,
                        if is_logging_in() {
                            "登录中..."
                        } else {
                            "登录"
                        }
                    }
                }

                div { class: "login-footer",
                    p {
                        "💡 提示: 将 "
                        code { "id_ed25519.json" }
                        " 放在用户主目录可自动加载"
                    }
                    p {
                        "首次使用？运行 "
                        code { "cargo run --bin keygen" }
                        " 生成密钥"
                    }
                }
            }
        }
    }
}
