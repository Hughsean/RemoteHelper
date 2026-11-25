//! 应用外壳（根视图）：在登录页与会话页之间切换。

use crate::theme::window_tint;
use crate::ui::page_tab;
use crate::views::{Dashboard, Login, Services};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::*;
use tokio::runtime::Handle;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Services,
}

/// 应用根视图：未登录时渲染登录页，登录后渲染监控或服务页。
pub(crate) struct RemoteHelper {
    pub(crate) runtime: Handle,
    pub(crate) login: Login,
    session: Option<Session>,
}

struct Session {
    address: String,
    page: Page,
    dashboard: Entity<Dashboard>,
    services: Entity<Services>,
}

impl RemoteHelper {
    pub(crate) fn new(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            runtime,
            login: Login::new(window, cx),
            session: None,
        }
    }

    pub(crate) fn open_session(
        &mut self,
        address: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dashboard = cx.new(|cx| Dashboard::new(self.runtime.clone(), cx));
        let services = cx.new(|cx| Services::new(self.runtime.clone(), window, cx));
        self.session = Some(Session {
            address,
            page: Page::Dashboard,
            dashboard,
            services,
        });
        cx.notify();
    }

    fn show_services(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.page != Page::Services
        {
            session.page = Page::Services;
            session.services.update(cx, Services::refresh);
            cx.notify();
        }
    }

    fn show_dashboard(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session {
            session.page = Page::Dashboard;
            cx.notify();
        }
    }
}

impl Render for RemoteHelper {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.session {
            None => self.render_login(cx).into_any_element(),
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
                    ))
                    .child(page_tab(
                        !dashboard,
                        Button::new("services")
                            .label("服务")
                            .ghost()
                            .small()
                            .on_click(cx.listener(Self::show_services)),
                    ));
                let status = h_flex()
                    .h(px(28.))
                    .px_4()
                    .gap_2()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::from(IconName::Server).small())
                    .child(div().min_w_0().truncate().child(session.address.clone()));

                v_flex()
                    .size_full()
                    .child(toolbar)
                    .child(div().min_h_0().w_full().flex_1().child(if dashboard {
                        session.dashboard.clone().into_any_element()
                    } else {
                        session.services.clone().into_any_element()
                    }))
                    .child(status)
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .bg(window_tint())
            .text_color(cx.theme().foreground)
            .child(content)
    }
}
