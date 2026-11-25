#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod dashboard;
mod login;
mod services;
mod trend;

use common::Response;
use dashboard::Dashboard;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, Theme, ThemeMode,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::*;
use login::Login;
use services::Services;
use tokio::runtime::Handle;

// GPUI Kit's default bundle omits several Lucide icons used by this app.
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Activity,
        Server,
        Cpu,
        MemoryStick,
        Monitor,
        RefreshCw,
        Plus,
        ChevronDown
    ]
);

struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(data) => Ok(Some(data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ExtraIcons.list(path)?;
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    let _log = common::logging::tracing_init(None, None, tracing::Level::INFO);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法启动网络运行时");
    let handle = runtime.handle().clone();

    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            Theme::update(cx, |theme| {
                theme.background = workspace_background();
                theme.foreground = rgb(0xe7eeed).into();
                theme.muted_foreground = rgb(0xaeb9bd).into();
                theme.border = rgb(0x394344).into();
                theme.muted = rgb(0x283232).into();
                theme.input = rgb(0x394545).into();
                theme.ring = accent();
                theme.popover = surface();
                theme.popover_foreground = theme.foreground;
            });
            let result = gpui_kit::open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("RemoteHelper".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1080.), px(760.)),
                        cx,
                    ))),
                    #[cfg(target_os = "windows")]
                    window_background: WindowBackgroundAppearance::MicaBackdrop,
                    window_min_size: Some(size(px(780.), px(580.))),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| RemoteHelper::new(handle.clone(), window, cx)),
            );
            match result {
                Ok(_) => cx.activate(true),
                Err(error) => {
                    tracing::error!(%error, "无法打开 RemoteHelper 窗口");
                    cx.quit();
                }
            }
        });
}

#[derive(PartialEq, Eq)]
enum Page {
    Dashboard,
    Services,
}

struct RemoteHelper {
    runtime: Handle,
    login: Login,
    session: Option<Session>,
}

struct Session {
    address: String,
    page: Page,
    dashboard: Entity<Dashboard>,
    services: Entity<Services>,
}

impl RemoteHelper {
    fn new(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            runtime,
            login: Login::new(window, cx),
            session: None,
        }
    }

    fn open_session(&mut self, address: String, window: &mut Window, cx: &mut Context<Self>) {
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

fn page_tab(active: bool, button: Button) -> Div {
    v_flex()
        .relative()
        .h_full()
        .items_center()
        .justify_center()
        .child(button)
        .children(active.then(|| {
            div()
                .absolute()
                .bottom_0()
                .w(px(18.))
                .h(px(2.))
                .rounded_full()
                .bg(accent())
        }))
}

fn window_tint() -> Hsla {
    #[cfg(target_os = "windows")]
    {
        workspace_background().opacity(0.76)
    }
    #[cfg(not(target_os = "windows"))]
    {
        workspace_background()
    }
}

fn workspace_background() -> Hsla {
    rgb(0x1b2222).into()
}

fn surface() -> Hsla {
    rgb(0x283031).into()
}

fn accent() -> Hsla {
    rgb(0x70dbc0).into()
}

fn nav_variant(active: bool, cx: &App) -> ButtonVariant {
    if active {
        let accent = accent();
        ButtonVariant::Custom(
            ButtonCustomVariant::new(cx)
                .color(accent)
                .foreground(accent)
                .hover(accent.opacity(0.24))
                .active(accent.opacity(0.30)),
        )
    } else {
        ButtonVariant::Ghost
    }
}

fn brand_mark() -> Div {
    let accent = accent();
    div()
        .size(px(32.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(accent.opacity(0.14))
        .text_color(accent)
        .child(Icon::from(IconName::Activity).small())
}

fn small_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .with_variant(nav_variant(active, cx))
}

fn panel(cx: &App) -> Div {
    v_flex()
        .min_w_0()
        .gap_3()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(surface())
}

fn field(label: &'static str, element: impl IntoElement) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_sm().font_medium().child(label))
        .child(element)
}

fn message(text: impl Into<SharedString>, error: bool, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .text_sm()
        .text_color(if error {
            theme.danger
        } else {
            theme.muted_foreground
        })
        .child(text.into())
}

fn request_error(response: Response) -> String {
    match response {
        Response::Error(error) => error,
        _ => "服务器返回了意外响应".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::AppAssets;
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconName;

    #[test]
    fn application_icons_load_from_the_asset_source() {
        for icon in [
            IconName::Activity,
            IconName::Server,
            IconName::Cpu,
            IconName::MemoryStick,
            IconName::Monitor,
            IconName::RefreshCw,
            IconName::Plus,
            IconName::ChevronDown,
            IconName::Check,
        ] {
            let path = icon.path();
            let svg = AppAssets
                .load(path.as_ref())
                .expect("icon path should exist");
            assert!(svg.is_some_and(|bytes| !bytes.is_empty()), "missing {path}");
        }
    }
}
