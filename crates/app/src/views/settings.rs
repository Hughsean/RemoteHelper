use crate::preferences::{Appearance, ClientPreferences, Motion, RefreshRate, StartPage};
use crate::theme;
use crate::ui::{nav_variant, panel};
use crate::views::history_settings::{
    HistoryPreferences, HistorySection, HistorySettings, HistorySettingsChange,
};
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::*;
use tokio::runtime::Handle;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Section {
    #[default]
    Appearance,
    Monitor,
    LocalCache,
    Server,
    About,
}

impl Section {
    const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Monitor,
        Self::LocalCache,
        Self::Server,
        Self::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "外观与动效",
            Self::Monitor => "启动与监控",
            Self::LocalCache => "本地缓存",
            Self::Server => "远程服务器",
            Self::About => "关于",
        }
    }
}

pub(crate) struct SettingsSession {
    pub(crate) address: String,
    pub(crate) fingerprint: Option<String>,
    pub(crate) offline: bool,
    pub(crate) history: Entity<HistorySettings>,
}

struct SavingPreferences;

impl Global for SavingPreferences {}

pub(crate) struct Settings {
    runtime: Handle,
    section: Section,
    session: Option<SettingsSession>,
    local_history: Entity<HistorySettings>,
    local_loading: bool,
    local_error: Option<String>,
    save_error: Option<String>,
}

impl Settings {
    pub(crate) fn new(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.weak_entity();
        let local_history = cx.new(|cx| {
            HistorySettings::new(
                runtime.clone(),
                String::new(),
                None,
                None,
                HistoryPreferences::default(),
                move |change, _, cx| {
                    if matches!(change, HistorySettingsChange::Preferences(_)) {
                        let _ = settings.update(cx, |view, cx| {
                            view.local_error = None;
                            cx.notify();
                        });
                    }
                },
                window,
                cx,
            )
        });
        let mut settings = Self {
            runtime,
            section: Section::default(),
            session: None,
            local_history,
            local_loading: false,
            local_error: None,
            save_error: None,
        };
        settings.load_local_history(window, cx);
        settings
    }

    pub(crate) fn set_session(
        &mut self,
        session: Option<SettingsSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session = session;
        if self.session.is_none() {
            self.load_local_history(window, cx);
        }
        cx.notify();
    }

    fn load_local_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.local_loading {
            return;
        }
        self.local_loading = true;
        self.local_error = None;
        let task = self.runtime.spawn(HistoryPreferences::load());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
            let _ = this.update_in(cx, |view, window, cx| {
                match loaded {
                    Ok(preferences) => view.local_history.update(cx, |history, cx| {
                        history.apply_preferences(preferences, window, cx);
                    }),
                    Err(error) => view.local_error = Some(error),
                }
                view.local_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn opened(&self, window: &mut Window, cx: &mut Context<Self>) {
        let (section, history) = match self.section {
            Section::LocalCache => (
                HistorySection::Cache,
                self.session
                    .as_ref()
                    .map_or(&self.local_history, |session| &session.history),
            ),
            Section::Server => {
                let Some(session) = &self.session else {
                    return;
                };
                (HistorySection::Server, &session.history)
            }
            _ => return,
        };
        history.update(cx, |view, cx| view.open_section(section, window, cx));
    }

    fn save(&mut self, preferences: ClientPreferences, cx: &mut Context<Self>) {
        if preferences == ClientPreferences::get(cx) || cx.has_global::<SavingPreferences>() {
            return;
        }
        cx.set_global(SavingPreferences);
        self.save_error = None;
        let task = self.runtime.spawn(preferences.save());
        cx.spawn(async move |this, cx| {
            let result = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = cx.update(|cx| {
                cx.remove_global::<SavingPreferences>();
                if result.is_ok() {
                    preferences.install(cx);
                }
                cx.refresh_windows();
            });
            let _ = this.update(cx, |view, cx| {
                view.save_error = result.err();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn choices<T: Copy + PartialEq + 'static>(
        title: &'static str,
        description: &'static str,
        values: &[(T, &'static str)],
        selected: T,
        apply: fn(&mut ClientPreferences, T),
        available: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let disabled = !available || cx.has_global::<SavingPreferences>();
        panel(cx)
            .gap_2()
            .child(div().font_semibold().child(title))
            .child(div().text_sm().text_color(muted).child(description))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(
                        values
                            .iter()
                            .copied()
                            .enumerate()
                            .map(|(index, (value, label))| {
                                Button::new((title, index))
                                    .small()
                                    .with_variant(nav_variant(value == selected, cx))
                                    .label(label)
                                    .accessibility_label(if value == selected {
                                        format!("{label}，当前选项")
                                    } else {
                                        label.to_owned()
                                    })
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        let mut preferences = ClientPreferences::get(cx);
                                        apply(&mut preferences, value);
                                        view.save(preferences, cx);
                                    }))
                            }),
                    ),
            )
    }

    fn appearance(cx: &mut Context<Self>) -> Div {
        let current = ClientPreferences::get(cx);
        let frosted_supported = theme::supports_frosted();
        v_flex()
            .gap_3()
            .child(Self::choices(
                "配色主题",
                "应用到客户端的页面和控件。",
                &[
                    (Appearance::System, "跟随系统"),
                    (Appearance::Light, "浅色"),
                    (Appearance::Dark, "深色"),
                ],
                current.appearance,
                |prefs, value| prefs.appearance = value,
                true,
                cx,
            ))
            .child(Self::choices(
                "窗口磨砂",
                if frosted_supported {
                    "使用系统的背景材质。关闭后，窗口使用纯色底面。"
                } else {
                    "当前系统或图形合成方式使用纯色底面。"
                },
                &[(true, "开启"), (false, "关闭")],
                current.frosted && frosted_supported,
                |prefs, value| prefs.frosted = value,
                frosted_supported,
                cx,
            ))
            .child(Self::choices(
                "界面动画",
                "影响页面切换、趋势图缩放和实时过渡。",
                &[
                    (Motion::System, "跟随系统"),
                    (Motion::Allow, "开启"),
                    (Motion::Reduce, "减少动画"),
                ],
                current.motion,
                |prefs, value| prefs.motion = value,
                true,
                cx,
            ))
    }

    fn monitor(cx: &mut Context<Self>) -> Div {
        let current = ClientPreferences::get(cx);
        v_flex()
            .gap_3()
            .child(Self::choices(
                "登录后打开",
                "成功连接主机后进入的页面。",
                &[(StartPage::Monitor, "监控"), (StartPage::Services, "服务")],
                current.start_page,
                |prefs, value| prefs.start_page = value,
                true,
                cx,
            ))
            .child(Self::choices(
                "默认刷新间隔",
                "监控页底栏仍可临时切换间隔或暂停刷新。",
                &RefreshRate::CHOICES,
                current.refresh_rate,
                |prefs, value| prefs.refresh_rate = value,
                true,
                cx,
            ))
    }

    fn local_cache(&self, cx: &App) -> Div {
        if let Some(session) = &self.session {
            return div().child(session.history.clone());
        }
        if self.local_loading {
            return panel(cx).child("正在读取缓存设置…");
        }
        v_flex()
            .gap_3()
            .children(self.local_error.as_ref().map(|error| {
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(format!("无法读取缓存设置：{error}"))
            }))
            .child(self.local_history.clone())
    }

    fn server(&self, cx: &App) -> Div {
        let Some(session) = &self.session else {
            return panel(cx)
                .child(div().font_semibold().child("当前未连接主机"))
                .child(
                    div()
                        .text_sm()
                        .child("登录后可在此查看服务器身份和历史记录设置。"),
                );
        };
        let mut connection = panel(cx)
            .gap_2()
            .child(div().font_semibold().child("当前主机"))
            .child(div().text_sm().child(session.address.clone()));
        if let Some(fingerprint) = &session.fingerprint {
            let fingerprint = fingerprint.clone();
            connection = connection
                .child(div().text_sm().child("已验证的服务器指纹"))
                .child(div().text_xs().child(fingerprint.clone()))
                .child(
                    Button::new("copy-server-fingerprint")
                        .label("复制指纹")
                        .small()
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(fingerprint.clone()));
                        }),
                );
        } else {
            connection = connection.child(div().text_sm().child(if session.offline {
                "正在离线查看本地历史。"
            } else {
                "此会话未提供可验证的服务器指纹。"
            }));
        }
        v_flex()
            .gap_3()
            .child(connection)
            .children((!session.offline).then(|| session.history.clone()))
    }

    fn about(cx: &App) -> Div {
        panel(cx)
            .child(div().font_semibold().child("RemoteHelper"))
            .child(
                div()
                    .text_sm()
                    .child(concat!("版本 ", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("project-home")
                            .small()
                            .label("项目主页")
                            .on_click(|_, _, cx| {
                                cx.open_url("https://github.com/Hughsean/RemoteHelper")
                            }),
                    )
                    .child(
                        Button::new("project-issues")
                            .small()
                            .label("反馈问题")
                            .on_click(|_, _, cx| {
                                cx.open_url("https://github.com/Hughsean/RemoteHelper/issues")
                            }),
                    ),
            )
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = v_flex()
            .w(px(164.))
            .flex_shrink_0()
            .p_3()
            .gap_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_base()
                    .font_semibold()
                    .child("设置"),
            )
            .children(Section::ALL.map(|section| {
                Button::new(("settings-category", section as usize))
                    .small()
                    .with_variant(nav_variant(self.section == section, cx))
                    .label(section.title())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if view.section != section {
                            view.section = section;
                            view.opened(window, cx);
                            cx.notify();
                        }
                    }))
            }));
        let content = match self.section {
            Section::Appearance => Self::appearance(cx),
            Section::Monitor => Self::monitor(cx),
            Section::LocalCache => self.local_cache(cx),
            Section::Server => self.server(cx),
            Section::About => Self::about(cx),
        };
        let saving = cx.has_global::<SavingPreferences>();
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        h_flex()
            .size_full()
            .min_h_0()
            .items_stretch()
            .overflow_hidden()
            .child(sidebar)
            .child(
                div()
                    .id(("settings-content", self.section as usize))
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(800.))
                            .mx_auto()
                            .p_4()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_semibold()
                                    .child(self.section.title()),
                            )
                            .children(
                                saving.then(|| {
                                    div().text_sm().text_color(muted).child("正在保存设置…")
                                }),
                            )
                            .children(self.save_error.as_ref().map(|error| {
                                div()
                                    .text_sm()
                                    .text_color(danger)
                                    .child(format!("无法保存设置：{error}"))
                            }))
                            .child(content),
                    ),
            )
    }
}
