//! 应用外壳（根视图）：在登录页与会话页之间切换，并记住关闭前的界面。

use crate::notifications::Notices;
use crate::theme::window_tint;
use crate::ui::{ConnectionStatus, page_tab};
use crate::views::{
    Dashboard, Login, Services,
    history_settings::{HistoryPreferences, HistorySettings, HistorySettingsChange},
};
use client::history::{HistoryCache, HistorySync, SavedHistory};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    animation::cubic_bezier,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::*;
use std::{sync::Arc, time::Duration};
use tokio::runtime::Handle;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Services,
    Settings,
}

/// 关闭前的界面：存在即代表关闭时已登录，缺省代表停在登录页。
///
/// 登录凭据与连接保存在 `client` 的进程级全局里，关窗不会丢失，
/// 所以重开窗口时只要重建 [`Session`] 即可回到原来的页面。
#[derive(Clone)]
struct LastScreen {
    address: String,
    page: Page,
}

impl Global for LastScreen {}

impl LastScreen {
    fn remember(address: &str, page: Page, cx: &mut App) {
        cx.set_global(Self {
            address: address.to_string(),
            page,
        });
    }
}

/// 应用根视图：未登录时渲染登录页，登录后渲染监控或服务页。
pub(crate) struct RemoteHelper {
    pub(crate) runtime: Handle,
    pub(crate) login: Login,
    pub(crate) notices: Notices,
    session: Option<Session>,
    slide: Option<PageSlide>,
    offline_choices: Option<Vec<SavedHistory>>,
    opening_offline: bool,
    session_epoch: u64,
}

#[derive(Clone, Copy)]
struct PageSlide {
    id: usize,
    travel: Point<Pixels>,
    whole_view: bool,
}

struct Session {
    address: String,
    page: Page,
    dashboard: Entity<Dashboard>,
    services: Entity<Services>,
    settings: Entity<HistorySettings>,
    preferences: HistoryPreferences,
    remote: Option<HistoryRemote>,
    offline: Option<(HistoryCache, Option<i64>)>,
    sync: Option<Arc<HistorySync>>,
    connection_status: Entity<ConnectionStatus>,
}

#[derive(Clone)]
struct HistoryRemote {
    address: String,
    identity: common::server_auth::ServerIdentity,
    key: client::SigningKey,
    public_key: String,
}

impl HistoryRemote {
    fn sync(&self, preferences: &HistoryPreferences) -> Option<Arc<HistorySync>> {
        HistorySync::from_verified(
            self.address.clone(),
            &self.identity,
            self.key.clone(),
            self.public_key.clone(),
            preferences.cache,
        )
        .ok()
        .map(Arc::new)
    }
}

impl RemoteHelper {
    pub(crate) fn new(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let login = Login::new(window, cx);
        let mut this = Self {
            runtime,
            login,
            notices: Notices::new(window),
            session: None,
            slide: None,
            offline_choices: None,
            opening_offline: false,
            session_epoch: 0,
        };
        // 重开窗口时回到关闭前的界面，而不是从登录页重新开始。
        if let Some(remembered) = cx.try_global::<LastScreen>().cloned() {
            this.enter_session(remembered.address, remembered.page, window, cx);
        }
        this
    }

    pub(crate) fn open_session(
        &mut self,
        address: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enter_session(address, Page::Dashboard, window, cx);
        self.start_slide(point(px(0.), px(18.)), true);
    }

    fn enter_session(
        &mut self,
        address: String,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote_key = if let Ok(connection) = client::CONNECTION.try_lock() {
            let identity = connection
                .as_ref()
                .and_then(|c| c.server_identity())
                .cloned();
            let registered = client::SERVER_ADDRESS
                .lock()
                .ok()
                .is_some_and(|a| *a == address);
            let key = registered
                .then(|| client::SIGNING_KEY.lock().ok().and_then(|k| k.clone()))
                .flatten();
            key.map(|(key, public_key)| {
                let remote = identity.map(|identity| HistoryRemote {
                    address: address.clone(),
                    identity,
                    key,
                    public_key: public_key.clone(),
                });
                (remote, public_key)
            })
        } else {
            None
        };
        let (remote, public_key) = remote_key.unwrap_or_default();
        self.make_session(
            address,
            page,
            remote,
            None,
            public_key,
            HistoryPreferences::default(),
            window,
            cx,
        );
        let epoch = self.session_epoch;
        let task = self.runtime.spawn(HistoryPreferences::load());
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(preferences)) = task.await {
                let _ = this.update_in(cx, |view, window, cx| {
                    if view.session_epoch == epoch {
                        view.history_change(
                            HistorySettingsChange::Preferences(preferences),
                            window,
                            cx,
                        );
                    }
                });
            }
        })
        .detach();
    }

    fn make_session(
        &mut self,
        address: String,
        page: Page,
        remote: Option<HistoryRemote>,
        offline: Option<(HistoryCache, Option<i64>)>,
        public_key: String,
        preferences: HistoryPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_epoch = self.session_epoch.wrapping_add(1);
        let epoch = self.session_epoch;
        let root = cx.weak_entity();
        let sync = remote.as_ref().and_then(|remote| remote.sync(&preferences));
        let connection_status = cx.new(|_| {
            if offline.is_some() {
                ConnectionStatus::Offline
            } else {
                ConnectionStatus::Connecting
            }
        });
        let dashboard = cx.new(|cx| {
            Dashboard::new(
                self.runtime.clone(),
                self.notices,
                connection_status.clone(),
                cx,
            )
        });
        dashboard.update(cx, |view, cx| {
            view.set_polling(offline.is_none() && page == Page::Dashboard, cx);
            view.history_sources(
                self.runtime.clone(),
                sync.clone(),
                offline.as_ref().map(|(cache, _)| cache.clone()),
                offline.as_ref().and_then(|(_, anchor)| *anchor),
                preferences.trend_window,
                preferences.cache.refresh_interval,
                false,
                cx,
            );
        });
        let services = cx.new(|_| Services::new(self.runtime.clone(), self.notices));
        let settings = cx.new(|cx| {
            HistorySettings::new(
                self.runtime.clone(),
                public_key,
                sync.clone(),
                offline.as_ref().map(|(cache, _)| cache.clone()),
                preferences,
                move |change, window, cx| {
                    let _ = root.update(cx, |view, cx| {
                        if view.session_epoch == epoch {
                            view.history_change(change, window, cx);
                        }
                    });
                },
                window,
                cx,
            )
        });
        if page == Page::Services && offline.is_none() {
            services.update(cx, |view, cx| view.refresh(window, cx));
        }
        if page == Page::Settings {
            settings.update(cx, |view, cx| view.refresh(window, cx));
        }
        if offline.is_none() {
            LastScreen::remember(&address, page, cx);
        }
        self.offline_choices = None;
        self.session = Some(Session {
            address,
            page,
            dashboard,
            services,
            settings,
            preferences,
            remote,
            offline,
            sync,
            connection_status,
        });
        cx.notify();
    }

    fn history_change(
        &mut self,
        change: HistorySettingsChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        let preferences_changed = matches!(&change, HistorySettingsChange::Preferences(_));
        let clear = matches!(&change, HistorySettingsChange::CacheCleared);
        if let HistorySettingsChange::Preferences(preferences) = change {
            session.preferences = preferences;
            session.sync = session
                .remote
                .as_ref()
                .and_then(|remote| remote.sync(&session.preferences));
        }
        let cache = session.offline.as_ref().map(|(cache, _)| cache.clone());
        let anchor = session.offline.as_ref().and_then(|(_, anchor)| *anchor);
        if preferences_changed {
            session.settings.update(cx, |view, cx| {
                view.apply_preferences(session.preferences, window, cx);
                view.set_sources(session.sync.clone(), cache.clone(), cx);
                if session.page == Page::Settings {
                    view.refresh(window, cx);
                }
            });
        }
        session.dashboard.update(cx, |view, cx| {
            view.history_sources(
                self.runtime.clone(),
                session.sync.clone(),
                cache,
                anchor,
                session.preferences.trend_window,
                session.preferences.cache.refresh_interval,
                clear || preferences_changed,
                cx,
            )
        });
        cx.notify();
    }

    fn show_services(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.page != Page::Services
            && session.offline.is_none()
        {
            session.page = Page::Services;
            session
                .dashboard
                .update(cx, |view, cx| view.set_polling(false, cx));
            session
                .services
                .update(cx, |view, cx| view.refresh(window, cx));
            LastScreen::remember(&session.address, Page::Services, cx);
            self.start_slide(point(px(28.), px(0.)), false);
            cx.notify();
        }
    }

    fn show_dashboard(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.page != Page::Dashboard
        {
            session.page = Page::Dashboard;
            let online = session.offline.is_none();
            session
                .dashboard
                .update(cx, |view, cx| view.set_polling(online, cx));
            if online {
                LastScreen::remember(&session.address, Page::Dashboard, cx);
            }
            self.start_slide(point(px(-28.), px(0.)), false);
            cx.notify();
        }
    }

    fn show_settings(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.page != Page::Settings
        {
            session.page = Page::Settings;
            session
                .dashboard
                .update(cx, |view, cx| view.set_polling(false, cx));
            session
                .settings
                .update(cx, |view, cx| view.refresh(window, cx));
            if session.offline.is_none() {
                LastScreen::remember(&session.address, Page::Settings, cx);
            }
            self.start_slide(point(px(28.), px(0.)), false);
            cx.notify();
        }
    }

    fn list_offline(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_offline {
            return;
        }
        self.opening_offline = true;
        let task = self.runtime.spawn(HistoryCache::list_pinned());
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = this.update_in(cx, |view, _, cx| {
                view.opening_offline = false;
                if view.session.is_none() {
                    match result {
                        Ok(items) => view.offline_choices = Some(items),
                        Err(err) => view
                            .notices
                            .error(format!("无法读取离线历史列表：{err}"), cx),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_offline(&mut self, saved: SavedHistory, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_offline || self.session.is_some() {
            return;
        }
        self.opening_offline = true;
        let epoch = self.session_epoch;
        let address = saved.address.clone();
        let task = self.runtime.spawn(async move {
            let preferences = HistoryPreferences::load().await.unwrap_or_default();
            let cache =
                HistoryCache::open_saved(&saved.fingerprint, saved.store_id, preferences.cache)
                    .await?;
            let anchor = cache
                .latest_wall_ms()
                .await?
                .or(cache
                    .extent()
                    .await?
                    .map(|range| range.to.saturating_sub(1)))
                .or(saved.last_sync_ms);
            Ok::<_, client::history::HistoryError>((cache, anchor, preferences))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = this.update_in(cx, |view, window, cx| {
                view.opening_offline = false;
                if view.session_epoch == epoch && view.session.is_none() {
                    match result {
                        Ok((cache, anchor, preferences)) => view.make_session(
                            address,
                            Page::Dashboard,
                            None,
                            Some((cache, anchor)),
                            String::new(),
                            preferences,
                            window,
                            cx,
                        ),
                        Err(err) => view.notices.error(format!("无法打开离线历史：{err}"), cx),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn leave_session(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.session = None;
        self.slide = None;
        self.session_epoch = self.session_epoch.wrapping_add(1);
        cx.remove_global::<LastScreen>();
        cx.notify();
    }

    fn start_slide(&mut self, travel: Point<Pixels>, whole_view: bool) {
        self.slide = Some(PageSlide {
            id: self.slide.map_or(0, |slide| slide.id.wrapping_add(1)),
            travel,
            whole_view,
        });
    }
}

fn slide_in(content: AnyElement, slide: Option<PageSlide>, cx: &App) -> AnyElement {
    let Some(slide) = slide.filter(|_| !cx.reduce_motion()) else {
        return content;
    };
    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .child(div().absolute().size_full().child(content).with_animation(
            ("page-enter", slide.id),
            Animation::new(Duration::from_millis(250)).with_easing(cubic_bezier(0.1, 0.9, 0.2, 1.)),
            move |element, progress| {
                let remaining = 1. - progress;
                element
                    .left(slide.travel.x * remaining)
                    .top(slide.travel.y * remaining)
            },
        ))
        .into_any_element()
}

impl Render for RemoteHelper {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.session {
            None => v_flex()
                .size_full()
                .child(div().flex_1().min_h_0().child(self.render_login(cx)))
                .child(
                    v_flex()
                        .px_4()
                        .pb_3()
                        .gap_1()
                        .max_h(px(170.))
                        .id("offline-history-list")
                        .overflow_y_scroll()
                        .child(
                            Button::new("offline-history")
                                .label(if self.opening_offline {
                                    "正在读取缓存…"
                                } else {
                                    "离线历史"
                                })
                                .ghost()
                                .small()
                                .on_click(cx.listener(Self::list_offline)),
                        )
                        .children(
                            self.offline_choices
                                .as_ref()
                                .filter(|items| items.is_empty())
                                .map(|_| div().text_xs().child("暂无可离线查看的已验证历史缓存")),
                        )
                        .children(self.offline_choices.iter().flatten().enumerate().map(
                            |(index, saved)| {
                                let label = format!(
                                    "{} · {}… · {}",
                                    saved.address,
                                    &saved.fingerprint[..12],
                                    if saved.current_pin {
                                        "当前绑定"
                                    } else {
                                        "旧绑定"
                                    }
                                );
                                let saved = saved.clone();
                                Button::new(("offline-cache", index))
                                    .label(label)
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.open_offline(saved.clone(), window, cx)
                                    }))
                            },
                        )),
                )
                .into_any_element(),
            Some(session) => {
                let dashboard = session.page == Page::Dashboard;
                let border = cx.theme().border;
                let muted = cx.theme().muted_foreground;
                let toolbar = h_flex()
                    .h(px(40.))
                    .px_3()
                    .gap_2()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(border)
                    .child(page_tab(
                        dashboard,
                        Button::new("dashboard")
                            .label("监控")
                            .ghost()
                            .small()
                            .on_click(cx.listener(Self::show_dashboard)),
                        cx,
                    ))
                    .children(session.offline.is_none().then(|| {
                        page_tab(
                            session.page == Page::Services,
                            Button::new("services")
                                .label("服务")
                                .ghost()
                                .small()
                                .on_click(cx.listener(Self::show_services)),
                            cx,
                        )
                    }))
                    .child(page_tab(
                        session.page == Page::Settings,
                        Button::new("history-settings")
                            .label("历史设置")
                            .ghost()
                            .small()
                            .on_click(cx.listener(Self::show_settings)),
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(session.connection_status.clone())
                    .child(
                        Button::new("leave-session")
                            .label(if session.offline.is_some() {
                                "返回登录"
                            } else {
                                "切换主机"
                            })
                            .ghost()
                            .small()
                            .on_click(cx.listener(Self::leave_session)),
                    );
                let footer = h_flex()
                    .h(px(28.))
                    .px_4()
                    .gap_2()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::from(IconName::Server).small())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(session.address.clone()),
                    )
                    .children(
                        (dashboard && session.offline.is_none())
                            .then(|| Dashboard::interval_control(&session.dashboard, cx)),
                    );

                let page = match session.page {
                    Page::Dashboard => session.dashboard.clone().into_any_element(),
                    Page::Services => session.services.clone().into_any_element(),
                    Page::Settings => session.settings.clone().into_any_element(),
                };
                let content = v_flex()
                    .size_full()
                    .child(toolbar)
                    .child(div().min_h_0().w_full().flex_1().child(slide_in(
                        page,
                        self.slide.filter(|slide| !slide.whole_view),
                        cx,
                    )))
                    .child(footer)
                    .into_any_element();
                slide_in(content, self.slide.filter(|slide| slide.whole_view), cx)
            }
        };
        div()
            .size_full()
            .bg(window_tint(cx))
            .text_color(cx.theme().foreground)
            .child(content)
    }
}
