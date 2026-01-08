// The dioxus prelude contains a ton of common items used in dioxus apps. It's a good idea to import wherever you
// need dioxus
use dioxus::prelude::*;

use tracing::Level;
use views::{Dashboard, Login};

/// Define a components module that contains all shared components for our app.
mod components;
/// Define a views module that contains the UI for all Layouts and Routes for our app.
mod views;

/// The Route enum is used to define the structure of internal routes in our app. All route enums need to derive
/// the [`Routable`] trait, which provides the necessary methods for the router to work.
///
/// Each variant represents a different URL pattern that can be matched by the router. If that pattern is matched,
/// the components for that route will be rendered.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    // 登录页面（默认路由）
    #[route("/")]
    Login {},

    // 仪表板测试页面
    #[route("/dashboard")]
    Dashboard {},
}

// We can import assets in dioxus with the `asset!` macro. This macro takes a path to an asset relative to the crate root.
// The macro returns an `Asset` type that will display as the path to the asset in the browser or a local path in desktop bundles.
const FAVICON: Asset = asset!("/assets/icon.png");
// The asset macro also minifies some assets like CSS and JS to make bundled smaller
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {

    dioxus_logger::init(Level::DEBUG).expect("");

    #[cfg(feature = "desktop")]
    {
        // // The `launch` function is the main entry point for a dioxus app. It takes a component and renders it with the platform feature
        // // you have enabled

        let mut cfg = dioxus::desktop::Config::new().with_window(
            dioxus::desktop::WindowBuilder::new()
                .with_title("RemoteHelper - 远程系统监控")
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(800, 600))
                .with_min_inner_size(dioxus::desktop::LogicalSize::new(800, 600)),
        );

        // 仅在Windows上移除菜单栏
        #[cfg(target_os = "windows")]
        {
            dioxus_logger::tracing::info!("移除 Windows 菜单栏");
            cfg = cfg.with_menu(None);
        }

        // macOS 自定义中文菜单
        #[cfg(target_os = "macos")]
        {
            use dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

            let menu = Menu::new();

            // 应用菜单
            let app_submenu = Submenu::new("RemoteHelper", true);
            app_submenu
                .append(&PredefinedMenuItem::about(Some("关于 RemoteHelper"), None))
                .ok();
            app_submenu.append(&PredefinedMenuItem::separator()).ok();
            app_submenu
                .append(&PredefinedMenuItem::hide(Some("隐藏")))
                .ok();
            app_submenu
                .append(&PredefinedMenuItem::hide_others(Some("隐藏其他")))
                .ok();
            app_submenu
                .append(&PredefinedMenuItem::show_all(Some("显示全部")))
                .ok();
            app_submenu.append(&PredefinedMenuItem::separator()).ok();
            app_submenu
                .append(&PredefinedMenuItem::quit(Some("退出")))
                .ok();
            menu.append(&app_submenu).ok();

            // 窗口菜单
            let window_submenu = Submenu::new("窗口", true);
            window_submenu
                .append(&PredefinedMenuItem::minimize(Some("最小化")))
                .ok();
            window_submenu
                .append(&PredefinedMenuItem::maximize(Some("最大化")))
                .ok();
            window_submenu.append(&PredefinedMenuItem::separator()).ok();
            window_submenu
                .append(&PredefinedMenuItem::close_window(Some("关闭窗口")))
                .ok();
            menu.append(&window_submenu).ok();

            cfg = cfg.with_menu(menu);
        }

        // println!("[4/4] 正在启动 Dioxus 应用...");
        // dioxus_logger::tracing::info!("正在启动 Dioxus 应用...");
        dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
        // println!("[✓] 应用已正常退出");
        // dioxus_logger::tracing::info!("应用已退出");
        // dioxus::launch(App);
    }
}

/// App is the main component of our app. Components are the building blocks of dioxus apps. Each component is a function
/// that takes some props and returns an Element. In this case, App takes no props because it is the root of our app.
///
/// Components should be annotated with `#[component]` to support props, better error messages, and autocomplete
#[component]
fn App() -> Element {
    // The `rsx!` macro lets us define HTML inside of rust. It expands to an Element with all of our HTML inside.
    rsx! {
        // In addition to element and text (which we will see later), rsx can contain other components. In this case,
        // we are using the `document::Link` component to add a link to our favicon and main CSS file into the head of our app.
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }

        // The router component renders the route enum we defined above. It will handle synchronization of the URL and render
        // the layouts and components for the active route.
        Router::<Route> {}
    }
}
