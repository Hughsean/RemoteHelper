use crate::shell::RemoteHelper;
use crate::ui::{brand_mark, field, message, nav_variant, panel};
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct LoginSettings {
    server_address: String,
    key_path: Option<PathBuf>,
}

impl Default for LoginSettings {
    fn default() -> Self {
        Self {
            server_address: "frp-try.com:53460".to_string(),
            key_path: None,
        }
    }
}

impl LoginSettings {
    fn path() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("remotehelper")
                .join("login_data.json"),
        )
    }

    fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    async fn save(&self) -> Result<(), String> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        let dir = path.parent().expect("login settings directory");
        let json = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|error| error.to_string())?;
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, json)
            .await
            .map_err(|error| error.to_string())?;
        tokio::fs::rename(tmp, path)
            .await
            .map_err(|error| error.to_string())
    }
}

pub(crate) struct Login {
    address: Entity<InputState>,
    password: Entity<InputState>,
    key_path: Option<PathBuf>,
    choosing_key: bool,
    pending: bool,
    error: Option<String>,
    _enter: Subscription,
}

impl Login {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RemoteHelper>) -> Self {
        let settings = LoginSettings::load();
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("host:port")
                .default_value(settings.server_address)
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("密钥文件密码")
                .masked(true)
        });
        let enter = cx.subscribe_in(&password, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.start_login(window, cx);
            }
        });
        let key_path = settings.key_path.filter(|path| path.is_file()).or_else(|| {
            let path = dirs::home_dir()?.join("id_ed25519.json");
            path.is_file().then_some(path)
        });
        Self {
            address,
            password,
            key_path,
            choosing_key: false,
            pending: false,
            error: None,
            _enter: enter,
        }
    }
}

impl RemoteHelper {
    fn choose_key(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.login.choosing_key = true;
        cx.notify();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let result = selection
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            _ = this.update(cx, |view, cx| {
                view.login.choosing_key = false;
                match result {
                    Ok(Some(paths)) => {
                        view.login.key_path = paths.into_iter().next();
                        view.login.error = None;
                    }
                    Ok(None) => {}
                    Err(error) => view.login.error = Some(format!("无法打开文件选择器：{error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.login.pending || self.login.choosing_key {
            return;
        }
        let address = self.login.address.read(cx).value().trim().to_string();
        let password = self.login.password.read(cx).value().to_string();
        let key_path = match (
            &self.login.key_path,
            address.is_empty(),
            password.is_empty(),
        ) {
            (_, true, _) => return self.login_error("请输入服务器地址", cx),
            (None, _, _) => return self.login_error("请选择密钥文件", cx),
            (_, _, true) => return self.login_error("请输入密钥文件密码", cx),
            (Some(path), false, false) => path.clone(),
        };

        self.login.pending = true;
        self.login.error = None;
        cx.notify();

        let settings = LoginSettings {
            server_address: address.clone(),
            key_path: Some(key_path),
        };
        let task = self.runtime.spawn(authenticate(settings, password));
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| format!("连接任务失败：{error}"))
                .and_then(|result| result);
            _ = this.update_in(cx, |view, window, cx| {
                view.login.pending = false;
                match result {
                    Ok(()) => {
                        view.login
                            .password
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        view.open_session(address, window, cx);
                    }
                    Err(error) => view.login_error(error, cx),
                }
            });
        })
        .detach();
    }

    fn login_error(&mut self, error: impl Into<String>, cx: &mut Context<Self>) {
        self.login.error = Some(error.into());
        cx.notify();
    }

    pub(crate) fn render_login(&self, cx: &mut Context<Self>) -> Div {
        let login = &self.login;
        let pending = login.pending;
        let disabled = pending || login.choosing_key;
        let filename = login
            .key_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未选择文件".to_string());
        let muted = cx.theme().muted_foreground;
        let card = panel(cx)
            .w(px(420.))
            .p_6()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .pb_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(brand_mark())
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_semibold().child("RemoteHelper"))
                            .child(div().text_sm().text_color(muted).child("连接远程服务器")),
                    ),
            )
            .child(field(
                "服务器地址",
                Input::new(&login.address).disabled(disabled),
            ))
            .child(field(
                "密钥文件",
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("select-key")
                            .label("选择文件")
                            .disabled(disabled)
                            .on_click(cx.listener(Self::choose_key)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(muted)
                            .child(filename),
                    ),
            ))
            .child(field(
                "密码",
                Input::new(&login.password)
                    .content_type(InputContentType::Password)
                    .disabled(disabled),
            ))
            .children(
                login
                    .error
                    .as_ref()
                    .map(|error| message(error.clone(), true, cx)),
            )
            .child(
                Button::new("login")
                    .with_variant(nav_variant(true, cx))
                    .w_full()
                    .disabled(disabled)
                    .label(if pending { "正在连接…" } else { "连接" })
                    .on_click(cx.listener(|view, _, window, cx| view.start_login(window, cx))),
            )
            .child(message(
                "将 id_ed25519.json 放在用户主目录可自动加载。",
                false,
                cx,
            ));

        #[cfg(debug_assertions)]
        let card = card.child(
            Button::new("demo")
                .ghost()
                .label("使用模拟数据")
                .disabled(disabled)
                .on_click(cx.listener(|view, _, window, cx| {
                    client::MOCK_MODE.store(true, Ordering::Relaxed);
                    view.open_session("模拟数据".to_string(), window, cx);
                })),
        );

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .child(card)
    }
}

async fn authenticate(settings: LoginSettings, password: String) -> Result<(), String> {
    if let Some(key_path) = &settings.key_path {
        let content = tokio::fs::read_to_string(key_path)
            .await
            .map_err(|error| format!("无法读取密钥文件：{error}"))?;
        let key: common::keys::KeyFile =
            serde_json::from_str(&content).map_err(|error| format!("密钥文件格式错误：{error}"))?;
        let signing_key = common::keys::decrypt_private_key(&key, &password)?;
        *client::SIGNING_KEY.lock().map_err(|_| "无法更新签名密钥")? =
            Some((signing_key, key.pub_key));
        *client::SERVER_ADDRESS
            .lock()
            .map_err(|_| "无法更新服务器地址")? = settings.server_address.clone();
    }
    client::MOCK_MODE.store(false, Ordering::Relaxed);
    let connection = tokio::time::timeout(Duration::from_secs(15), client::connect_and_auth())
        .await
        .map_err(|_| "连接超时".to_string())?
        .map_err(|error| format!("连接失败：{error}"))?;
    *client::CONNECTION.lock().await = Some(connection);
    if let Err(error) = settings.save().await {
        tracing::warn!(%error, "无法保存登录设置");
    }
    Ok(())
}
