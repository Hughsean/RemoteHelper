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

#[derive(Clone, Serialize, Deserialize)]
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

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
struct IdentityPrompt {
    address: String,
    fingerprint: String,
    previous: Option<String>,
    deployment_pin: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum LoginTrust {
    Legacy,
    Pinned(String),
    Confirm(String),
    Rebind {
        previous: String,
        fingerprint: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum LoginStep {
    Connect(LoginTrust),
    Review(IdentityPrompt),
}

pub(crate) struct Login {
    address: Entity<InputState>,
    password: Entity<InputState>,
    deployment_pin: Entity<InputState>,
    key_path: Option<PathBuf>,
    choosing_key: bool,
    pending: bool,
    identity_prompt: Option<IdentityPrompt>,
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
        let deployment_pin =
            cx.new(|cx| InputState::new(window, cx).placeholder("可选：部署方提供的 SHA-256 指纹"));
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
            deployment_pin,
            key_path,
            choosing_key: false,
            pending: false,
            identity_prompt: None,
            _enter: enter,
        }
    }
}

impl RemoteHelper {
    fn choose_key(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.login.pending || self.login.choosing_key || self.login.identity_prompt.is_some() {
            return;
        }
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
        if self.login.pending || self.login.choosing_key || self.login.identity_prompt.is_some() {
            return;
        }
        self.run_login(None, window, cx);
    }

    fn take_identity_prompt(&mut self, shown: &IdentityPrompt) -> bool {
        if self.login.identity_prompt.as_ref() != Some(shown) {
            return false;
        }
        self.login.identity_prompt = None;
        true
    }

    fn confirm_identity(
        &mut self,
        prompt: &IdentityPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.login.pending || self.login.choosing_key || !self.take_identity_prompt(prompt) {
            return;
        }
        let address = self.login.address.read(cx).value();
        let deployment_pin = self.login.deployment_pin.read(cx).value();
        if prompt.address != address.trim()
            || prompt.deployment_pin.as_deref().unwrap_or_default() != deployment_pin.trim()
        {
            self.notices
                .warning("服务器地址或部署指纹已更改，请重新连接并核对指纹", cx);
            cx.notify();
            return;
        }
        let trust = match &prompt.previous {
            Some(previous) => LoginTrust::Rebind {
                previous: previous.clone(),
                fingerprint: prompt.fingerprint.clone(),
            },
            None => LoginTrust::Confirm(prompt.fingerprint.clone()),
        };
        self.run_login(Some(trust), window, cx);
    }

    fn run_login(
        &mut self,
        trust: Option<LoginTrust>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        let address = self.login.address.read(cx).value().trim().to_string();
        let password = self.login.password.read(cx).value();
        let deployment_pin = self
            .login
            .deployment_pin
            .read(cx)
            .value()
            .trim()
            .to_string();
        let deployment_pin = (!deployment_pin.is_empty()).then_some(deployment_pin);
        if address.is_empty() {
            return self.notices.warning("请输入服务器地址", cx);
        }
        let Some(key_path) = self.login.key_path.clone() else {
            return self.notices.warning("请选择密钥文件", cx);
        };
        if password.is_empty() {
            return self.notices.warning("请输入密钥文件密码", cx);
        }
        if deployment_pin
            .as_deref()
            .is_some_and(|pin| !valid_fingerprint(pin))
        {
            return self
                .notices
                .warning("部署指纹须为 64 位小写十六进制数字", cx);
        }

        self.login.pending = true;
        let settings = LoginSettings {
            server_address: address.clone(),
            key_path: Some(key_path.clone()),
        };
        let task = self.runtime.spawn(attempt_login(
            settings.clone(),
            password.to_string(),
            deployment_pin.clone(),
            trust,
        ));
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| format!("连接任务失败：{error}"))
                .and_then(|result| result);
            _ = this.update_in(cx, |view, window, cx| {
                view.login.pending = false;
                cx.notify();
                if view.login.address.read(cx).value().trim() != address
                    || view.login.password.read(cx).value() != password
                    || view.login.key_path.as_ref() != Some(&key_path)
                    || view.login.deployment_pin.read(cx).value().trim()
                        != deployment_pin.as_deref().unwrap_or_default()
                {
                    return view
                        .notices
                        .warning("登录信息已更改，请重新连接并核对服务器身份", cx);
                }
                match result {
                    Ok(LoginResult::Review(prompt)) => view.login.identity_prompt = Some(prompt),
                    Ok(LoginResult::Connected(connection)) => {
                        if let Err(error) = connection.install(&address) {
                            return view.notices.error(error, cx);
                        }
                        for input in [&view.login.password, &view.login.deployment_pin] {
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        view.open_session(address, window, cx);
                        let task = view.runtime.spawn(async move { settings.save().await });
                        let notices = view.notices;
                        cx.spawn(async move |_, cx| {
                            let result = task
                                .await
                                .map_err(|error| error.to_string())
                                .and_then(|result| result);
                            if let Err(error) = result {
                                tracing::warn!(%error, "无法保存登录设置");
                                _ = cx.update(|cx| {
                                    notices
                                        .warning(format!("已连接，无法保存登录设置：{error}"), cx)
                                });
                            }
                        })
                        .detach();
                    }
                    Err(error) => view.notices.error(error, cx),
                }
            });
        })
        .detach();
    }

    fn render_identity_prompt(prompt: &IdentityPrompt, cx: &Context<Self>) -> Div {
        let rebind = prompt.previous.is_some();
        let muted = cx.theme().muted_foreground;
        let shown = prompt.clone();
        let copied = shown.clone();
        let cancelled = shown.clone();
        let confirm = Button::new("confirm-identity");
        let confirm = if rebind {
            confirm.danger()
        } else {
            confirm.primary()
        };
        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("copy-fingerprint")
                    .label(if rebind {
                        "复制新指纹"
                    } else {
                        "复制指纹"
                    })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if view.login.identity_prompt.as_ref() == Some(&copied) {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                copied.fingerprint.clone(),
                            ));
                            view.notices.success("已复制指纹", cx);
                        }
                    })),
            )
            .child(
                confirm
                    .label(if rebind {
                        "重新绑定并连接"
                    } else {
                        "确认并连接"
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.confirm_identity(&shown, window, cx)
                    })),
            )
            .child(
                Button::new("cancel-identity")
                    .ghost()
                    .label("取消")
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if view.take_identity_prompt(&cancelled) {
                            cx.notify();
                        }
                    })),
            );
        let heading = if rebind {
            "服务器身份已变更"
        } else {
            "首次连接：确认服务器身份"
        };
        let explanation = if rebind {
            "新指纹与已固定的指纹不符，可能连到了另一台服务器。请通过可信渠道独立核对新指纹；重新绑定会替换旧指纹。"
        } else {
            "请通过可信渠道（例如联系部署方）核对完整指纹，再确认并连接。"
        };
        let fingerprint_label = if rebind {
            "新指纹（SHA-256）"
        } else {
            "服务器指纹（SHA-256）"
        };
        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(div().font_semibold().child(heading))
            .child(
                div()
                    .text_sm()
                    .child(format!("目标地址：{}", prompt.address)),
            )
            .child(div().text_sm().text_color(muted).child(explanation))
            .children(
                prompt
                    .previous
                    .as_deref()
                    .map(|previous| fingerprint_row("已固定指纹", previous)),
            )
            .child(fingerprint_row(fingerprint_label, &prompt.fingerprint))
            .child(actions)
    }

    pub(crate) fn render_login(&self, cx: &mut Context<Self>) -> Div {
        let login = &self.login;
        let pending = login.pending;
        let disabled = pending || login.choosing_key || login.identity_prompt.is_some();
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
                    if view.login.pending
                        || view.login.choosing_key
                        || view.login.identity_prompt.is_some()
                    {
                        return;
                    }
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
            .children(login.identity_prompt.is_none().then(|| {
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
                    ))
                    .child(login_row(
                        "部署指纹",
                        Input::new(&login.deployment_pin)
                            .appearance(false)
                            .disabled(disabled),
                        stroke,
                    ))
            }))
            .children(
                login
                    .identity_prompt
                    .as_ref()
                    .map(|prompt| Self::render_identity_prompt(prompt, cx)),
            )
            .children(login.identity_prompt.is_none().then_some(actions));

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

fn fingerprint_row(label: &'static str, fingerprint: &str) -> Div {
    let (first, second) = fingerprint
        .split_at_checked(32)
        .unwrap_or((fingerprint, ""));
    v_flex()
        .gap_1()
        .child(div().text_xs().child(label))
        .child(div().text_size(px(12.)).child(first.to_owned()))
        .child(div().text_size(px(12.)).child(second.to_owned()))
}

fn valid_fingerprint(pin: &str) -> bool {
    pin.len() == 64
        && pin
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn login_step(
    address: &str,
    pinned: Option<String>,
    deployment_pin: Option<&str>,
    signed_fingerprint: Option<String>,
) -> Result<LoginStep, String> {
    let Some(fingerprint) = signed_fingerprint else {
        return if pinned.is_none() && deployment_pin.is_none() {
            Ok(LoginStep::Connect(LoginTrust::Legacy))
        } else {
            Err("此地址要求验证服务器身份，但服务端未提供签名，无法连接。".into())
        };
    };
    if deployment_pin.is_some_and(|pin| pin != fingerprint) {
        return Err("服务器指纹与部署指纹不符。请向部署方核对，无法连接。".into());
    }
    match pinned {
        Some(pinned) if pinned == fingerprint => {
            Ok(LoginStep::Connect(LoginTrust::Pinned(fingerprint)))
        }
        None if deployment_pin.is_some() => {
            Ok(LoginStep::Connect(LoginTrust::Confirm(fingerprint)))
        }
        previous => Ok(LoginStep::Review(IdentityPrompt {
            address: address.to_owned(),
            fingerprint,
            previous,
            deployment_pin: deployment_pin.map(str::to_owned),
        })),
    }
}

struct AuthenticatedLogin {
    connection: client::EncryptedConnection,
    signing_key: client::SigningKey,
    public_key: String,
}

impl AuthenticatedLogin {
    fn install(self, address: &str) -> Result<(), String> {
        // 按重连顺序获取所有锁；获取失败时不提交部分登录状态。
        let mut connection = client::CONNECTION
            .try_lock()
            .map_err(|_| "连接状态暂时不可用，请重试")?;
        let mut signing_key = client::SIGNING_KEY.lock().map_err(|_| "无法更新签名密钥")?;
        let mut server_address = client::SERVER_ADDRESS
            .lock()
            .map_err(|_| "无法更新服务器地址")?;
        *signing_key = Some((self.signing_key, self.public_key));
        *server_address = address.to_owned();
        *connection = Some(self.connection);
        client::MOCK_MODE.store(false, Ordering::Relaxed);
        Ok(())
    }
}

enum LoginResult {
    Review(IdentityPrompt),
    Connected(AuthenticatedLogin),
}

async fn attempt_login(
    settings: LoginSettings,
    password: String,
    deployment_pin: Option<String>,
    trust: Option<LoginTrust>,
) -> Result<LoginResult, String> {
    let address = &settings.server_address;
    let trust = match trust {
        Some(trust) => trust,
        None => {
            let pinned = client::load_server_pin(address)
                .map_err(|error| format!("无法读取服务器身份记录：{error}"))?;
            let probe = tokio::time::timeout(CONNECTION_TIMEOUT, client::probe_server(address))
                .await
                .map_err(|_| "探测服务器身份超时".to_string())?
                .map_err(|error| format!("无法验证服务器身份：{error}"))?;
            let signed_fingerprint = match probe {
                client::ServerProbe::History(identity) => Some(identity.fingerprint),
                client::ServerProbe::Legacy => None,
            };
            match login_step(
                address,
                pinned,
                deployment_pin.as_deref(),
                signed_fingerprint,
            )? {
                LoginStep::Review(prompt) => return Ok(LoginResult::Review(prompt)),
                LoginStep::Connect(trust) => trust,
            }
        }
    };
    let key_path = settings.key_path.ok_or("请选择密钥文件")?;
    let content = tokio::fs::read_to_string(key_path)
        .await
        .map_err(|error| format!("无法读取密钥文件：{error}"))?;
    let key: common::keys::KeyFile =
        serde_json::from_str(&content).map_err(|error| format!("密钥文件格式错误：{error}"))?;
    let signing_key = common::keys::decrypt_private_key(&key, &password)?;
    let public_key = key.pub_key;

    let confirming = matches!(&trust, LoginTrust::Confirm(_) | LoginTrust::Rebind { .. });
    let connection = tokio::time::timeout(CONNECTION_TIMEOUT, async {
        match trust {
            LoginTrust::Legacy => {
                client::connect_with_pin(address, None, &signing_key, &public_key).await
            }
            LoginTrust::Pinned(fingerprint) => {
                client::connect_with_pin(address, Some(&fingerprint), &signing_key, &public_key)
                    .await
            }
            LoginTrust::Confirm(fingerprint) => {
                client::confirm_server_identity(address, &fingerprint, &signing_key, &public_key)
                    .await
            }
            LoginTrust::Rebind {
                previous,
                fingerprint,
            } => {
                client::rebind_server_identity(
                    address,
                    &previous,
                    &fingerprint,
                    &signing_key,
                    &public_key,
                )
                .await
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("连接超时".to_string()))
    .map_err(|error| {
        if confirming {
            format!("未能完成身份确认和登录，请重新连接并核对完整指纹：{error}")
        } else {
            format!("连接失败，请重新连接并核对服务器身份：{error}")
        }
    })?;
    Ok(LoginResult::Connected(AuthenticatedLogin {
        connection,
        signing_key,
        public_key,
    }))
}

#[cfg(test)]
mod tests {
    use super::{IdentityPrompt, LoginStep, LoginTrust, login_step, valid_fingerprint};

    const ADDRESS: &str = "localhost:1234";
    const OLD: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const NEW: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn first_use_displays_complete_fingerprint() {
        assert_eq!(
            login_step(ADDRESS, None, None, Some(NEW.into())).unwrap(),
            LoginStep::Review(IdentityPrompt {
                address: ADDRESS.into(),
                fingerprint: NEW.into(),
                previous: None,
                deployment_pin: None,
            })
        );
    }

    #[test]
    fn changed_identity_always_requires_explicit_rebind() {
        for deployment_pin in [None, Some(NEW)] {
            assert_eq!(
                login_step(ADDRESS, Some(OLD.into()), deployment_pin, Some(NEW.into())).unwrap(),
                LoginStep::Review(IdentityPrompt {
                    address: ADDRESS.into(),
                    fingerprint: NEW.into(),
                    previous: Some(OLD.into()),
                    deployment_pin: deployment_pin.map(str::to_owned),
                })
            );
        }
    }

    #[test]
    fn pinned_and_deployment_fingerprints_take_distinct_paths() {
        assert_eq!(
            login_step(ADDRESS, Some(NEW.into()), None, Some(NEW.into())).unwrap(),
            LoginStep::Connect(LoginTrust::Pinned(NEW.into()))
        );
        assert_eq!(
            login_step(ADDRESS, None, Some(NEW), Some(NEW.into())).unwrap(),
            LoginStep::Connect(LoginTrust::Confirm(NEW.into()))
        );
        assert!(login_step(ADDRESS, None, Some(OLD), Some(NEW.into())).is_err());
        assert!(login_step(ADDRESS, Some(OLD.into()), Some(OLD), Some(NEW.into())).is_err());
    }

    #[test]
    fn legacy_requires_that_no_fingerprint_is_expected() {
        assert_eq!(
            login_step(ADDRESS, None, None, None).unwrap(),
            LoginStep::Connect(LoginTrust::Legacy)
        );
        assert!(login_step(ADDRESS, Some(OLD.into()), None, None).is_err());
        assert!(login_step(ADDRESS, None, Some(OLD), None).is_err());
    }

    #[test]
    fn deployment_fingerprint_requires_full_lowercase_hex() {
        assert!(valid_fingerprint(NEW));
        assert!(!valid_fingerprint(&NEW[..63]));
        assert!(!valid_fingerprint(&NEW.to_uppercase()));
        assert!(!valid_fingerprint(&NEW.replace('a', "g")));
    }
}
