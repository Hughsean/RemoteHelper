use crate::shell::RemoteHelper;
use crate::ui::{brand_mark, panel};
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, Sizable,
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
                    }
                    Ok(None) => {}
                    Err(error) => view
                        .notices
                        .error(format!("无法打开文件选择器：{error}"), cx),
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
            (_, true, _) => return self.notices.warning("请输入服务器地址", cx),
            (None, _, _) => return self.notices.warning("请选择密钥文件", cx),
            (_, _, true) => return self.notices.warning("请输入密钥文件密码", cx),
            (Some(path), false, false) => path.clone(),
        };

        self.login.pending = true;
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
                    Ok(save_error) => {
                        view.login
                            .password
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        view.open_session(address, window, cx);
                        if let Some(error) = save_error {
                            view.notices
                                .warning(format!("已连接，无法保存登录设置：{error}"), cx);
                        }
                    }
                    Err(error) => {
                        view.notices.error(error, cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
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
        let stroke = cx.theme().border;

        let actions = h_flex().gap_2().child(
            Button::new("login")
                .primary()
                .flex_1()
                .disabled(disabled)
                .label(if pending { "正在连接…" } else { "连接" })
                .on_click(cx.listener(|view, _, window, cx| view.start_login(window, cx))),
        );
        #[cfg(debug_assertions)]
        let actions = actions.child(
            Button::new("demo")
                .ghost()
                .label("使用模拟数据")
                .disabled(disabled)
                .on_click(cx.listener(|view, _, window, cx| {
                    client::MOCK_MODE.store(true, Ordering::Relaxed);
                    view.open_session("模拟数据".to_string(), window, cx);
                })),
        );

        let card = panel(cx)
            .w(px(492.))
            .max_w_full()
            .rounded(px(12.))
            .p_7()
            .gap_6()
            .child(
                h_flex().gap_4().child(brand_mark(cx)).child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(24.))
                                .font_semibold()
                                .child("RemoteHelper"),
                        )
                        .child(div().text_sm().text_color(muted).child("连接远程服务器")),
                ),
            )
            .child(
                v_flex()
                    .border_t_1()
                    .border_color(stroke)
                    .child(login_row(
                        "服务器地址",
                        Input::new(&login.address)
                            .appearance(false)
                            .disabled(disabled),
                        stroke,
                    ))
                    .child(login_row(
                        "密钥文件",
                        h_flex()
                            .w_full()
                            .h_full()
                            .gap_2()
                            .min_w_0()
                            .px_2()
                            .justify_between()
                            .child(
                                div()
                                    .truncate()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(filename),
                            )
                            .child(
                                Button::new("select-key")
                                    .small()
                                    .ghost()
                                    .flex_shrink_0()
                                    .label("选择文件")
                                    .tooltip("主目录中的 id_ed25519.json 会自动加载")
                                    .disabled(disabled)
                                    .on_click(cx.listener(Self::choose_key)),
                            ),
                        stroke,
                    ))
                    .child(login_row(
                        "密钥密码",
                        Input::new(&login.password)
                            .appearance(false)
                            .content_type(InputContentType::Password)
                            .disabled(disabled),
                        stroke,
                    )),
            )
            .child(actions);

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(card)
    }
}

fn login_row(label: &'static str, input: impl IntoElement, stroke: Hsla) -> Div {
    h_flex()
        .h(px(48.))
        .border_b_1()
        .border_color(stroke)
        .child(
            div()
                .w(px(90.))
                .flex_shrink_0()
                .text_sm()
                .font_medium()
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .h_full()
                .flex_1()
                .flex()
                .items_center()
                .child(input),
        )
}

async fn authenticate(settings: LoginSettings, password: String) -> Result<Option<String>, String> {
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
    let save_error = settings.save().await.err();
    if let Some(error) = &save_error {
        tracing::warn!(%error, "无法保存登录设置");
    }
    Ok(save_error)
}
