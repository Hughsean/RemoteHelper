// Dioxus prelude 包含了 Dioxus 应用中使用的许多常见项。在需要 Dioxus 的地方导入它是好的做法
use dioxus::prelude::*;

use views::{Home, Login, Services, Test};

/// 定义一个包含应用所有共享组件的组件模块。
mod components;
/// 定义一个包含应用所有布局和路由 UI 的视图模块。
mod views;

/// Route 枚举用于定义应用内部路由的结构。所有路由枚举都需要派生 [`Routable`] trait，它为路由器工作提供必要的方法。
///
/// 每个变体代表路由器可以匹配的不同 URL 模式。如果匹配该模式，将渲染该路由的组件。
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    // 登录页面（默认路由）
    #[route("/")]
    Login {},

    // 仪表板测试页面
    #[route("/home")]
    Home {},

    // 测试页面
    #[route("/test")]
    Test {},

    // 服务管理页面
    #[route("/services")]
    Services {},
}

// 我们可以使用 `asset!` 宏在 Dioxus 中导入资源。该宏接受相对于 crate 根目录的资源路径。
// 该宏返回一个 `Asset` 类型，它将在浏览器中显示为资源路径，或在桌面包中显示为本地路径。
// const FAVICON: Asset = asset!("/assets/favicon.svg");
// asset 宏还会压缩一些资源如 CSS 和 JS 以使包更小
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    // dioxus_logger::init(Level::DEBUG).expect("");
    let _guard = common::func::tracing_init(None, None);

    #[cfg(feature = "desktop")]
    {
        // `launch` 函数是 Dioxus 应用的主入口点。它接受一个组件，并使用您启用的平台功能渲染它

        let mut cfg = dioxus::desktop::Config::new().with_window(
            dioxus::desktop::WindowBuilder::new()
                .with_title("RemoteHelper - 远程系统监控")
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(800, 700))
                .with_min_inner_size(dioxus::desktop::LogicalSize::new(800, 700)),
        );

        // 仅在Windows上移除菜单栏
        #[cfg(target_os = "windows")]
        {
            tracing::info!("移除 Windows 菜单栏");
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
        dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
    }
}

/// App 是我们应用的主组件。组件是 Dioxus 应用的构建块。每个组件是一个函数，接受一些 props 并返回一个 Element。在这种情况下，App 不接受 props 因为它是应用的根。
///
/// 组件应该用 `#[component]` 注解以支持 props、更好的错误消息和自动完成
#[component]
fn App() -> Element {
    // `rsx!` 宏让我们在 Rust 中定义 HTML。它扩展为包含所有 HTML 的 Element。
    rsx! {
        // 除了元素和文本（我们稍后会看到），rsx 可以包含其他组件。在这种情况下，
        // 我们使用 `document::Link` 组件将链接添加到应用的头部，用于主 CSS 文件。
        // document::Link { rel: "svg", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }

        // 路由器组件渲染我们上面定义的路由枚举。它将处理 URL 的同步并渲染活动路由的布局和组件。
        Router::<Route> {}
    }
}
