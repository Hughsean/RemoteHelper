//! 应用外壳（根视图）：在登录页与会话页之间切换，并记住关闭前的界面。

use crate::notifications::Notices;
use crate::theme::window_tint;
use crate::ui::page_tab;
use crate::views::{Dashboard, Login, Services};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    animation::cubic_bezier,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::*;
use std::time::Duration;
use tokio::runtime::Handle;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Services,
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
        let dashboard = cx.new(|cx| Dashboard::new(self.runtime.clone(), cx));
        let services = cx.new(|_| Services::new(self.runtime.clone(), self.notices));
        if page == Page::Services {
            services.update(cx, |view, cx| view.refresh(window, cx));
        }
        LastScreen::remember(&address, page, cx);
        self.session = Some(Session {
            address,
            page,
            dashboard,
            services,
        });
        cx.notify();
    }

    fn show_services(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.session
            && session.page != Page::Services
        {
            session.page = Page::Services;
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
            LastScreen::remember(&session.address, Page::Dashboard, cx);
            self.start_slide(point(px(-28.), px(0.)), false);
            cx.notify();
        }
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
                        cx,
                    ))
                    .child(page_tab(
                        !dashboard,
                        Button::new("services")
                            .label("服务")
                            .ghost()
                            .small()
                            .on_click(cx.listener(Self::show_services)),
                        cx,
                    ));
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
                    );

                let page = if dashboard {
                    session.dashboard.clone().into_any_element()
                } else {
                    session.services.clone().into_any_element()
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
