use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use gloo_storage::{LocalStorage, Storage};

const LOGIN_CSS: Asset = asset!("/assets/styling/login.css");

// 登录数据序列化结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoginData {
    server_address: String,
    key_file_name: String,
    password: String,
}

impl Default for LoginData {
    fn default() -> Self {
        Self {
            server_address: "frp-try.com:53460".to_string(),
            key_file_name: String::new(),
            password: String::new(),
        }
    }
}

#[component]
pub fn Login() -> Element {
    // 从本地存储/文件读取之前保存的数据
    let stored_data = load_login_data();

    let mut server_address = use_signal(move || stored_data.server_address.clone());
    let mut password = use_signal(move || stored_data.password.clone());
    let mut key_file_content = use_signal(|| Option::<String>::None);
    let mut key_file_name = use_signal(move || {
        if stored_data.key_file_name.is_empty() {
            "未选择文件".to_string()
        } else {
            stored_data.key_file_name.clone()
        }
    });
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
                        key_file_name.set(file.name().clone());
                        error_msg.set(None);

                        // 保存到本地存储
                        save_login_data(server_address(), file.name(), password());
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

                    // 保存登录数据到本地存储
                    save_login_data(server_address(), key_file_name(), password());

                    // 跳转到仪表板测试页面
                    navigator.push("/home");
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
                        oninput: move |evt| {
                            server_address.set(evt.value());
                            save_login_data(evt.value(), key_file_name(), password());
                        },
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
                        oninput: move |evt| {
                            password.set(evt.value());
                            save_login_data(server_address(), key_file_name(), evt.value());
                        },
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

                if cfg!(debug_assertions) {
                    hr {}
                    p { class: "debug-hint", "🔧 调试模式 — 无需服务器即可查看 UI" }
                    button {
                        class: "btn-debug-login",
                        onclick: move |_| {
                            client::MOCK_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
                            navigator.push("/home");
                        },
                        "调试登录（跳过认证）"
                    }
                }
            }
        }
    }
}

// 平台无关的加载/保存函数
#[cfg(target_arch = "wasm32")]
fn load_login_data() -> LoginData {
    LocalStorage::get("login_data").unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn load_login_data() -> LoginData {
    if let Some(mut p) = dirs::config_dir() {
        p.push("remotehelper");
        p.push("login_data.json");
        match std::fs::read_to_string(&p) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(ld) => ld,
                Err(e) => {
                    tracing::warn!("解析登录数据失败: {}", e);
                    LoginData::default()
                }
            },
            Err(_) => LoginData::default(),
        }
    } else {
        LoginData::default()
    }
}

#[cfg(target_arch = "wasm32")]
fn save_login_data(server_address: String, key_file_name: String, password: String) {
    let login_data = LoginData {
        server_address,
        key_file_name,
        password,
    };
    let _ = LocalStorage::set("login_data", &login_data);
}

#[cfg(not(target_arch = "wasm32"))]
fn save_login_data(server_address: String, key_file_name: String, password: String) {
    let login_data = LoginData {
        server_address,
        key_file_name,
        password,
    };
    if let Some(mut p) = dirs::config_dir() {
        p.push("remotehelper");
        if let Err(e) = std::fs::create_dir_all(&p) {
            tracing::warn!("无法创建配置目录 {:?}: {}", p, e);
            return;
        }
        p.push("login_data.json");
        match serde_json::to_string(&login_data) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&p, s) {
                    tracing::warn!("无法写入登录数据 {:?}: {}", p, e);
                }
            }
            Err(e) => tracing::warn!("序列化登录数据失败: {}", e),
        }
    }
}
