// dioxus prelude 包含了大量在 dioxus 应用中常用的项。在任何需要 dioxus 的地方导入它都是个好主意
use dioxus::prelude::*;

use views::Home;

/// 后端通信模块 - 封装与服务器的所有 API 调用
mod backend;
/// 定义一个包含应用所有共享组件的 components 模块
mod components;
/// 状态管理模块 - 使用 Signal 和 Context 实现响应式状态
mod state;
/// 定义一个包含应用所有布局和路由 UI 的 views 模块
mod views;

use state::{AuthData, ServicesData, SystemData};

/// Route 枚举用于定义应用中内部路由的结构。所有路由枚举都需要派生 [`Routable`] trait，
/// 它提供了路由器工作所需的必要方法。
///
/// 每个变体代表一个不同的 URL 模式，可以被路由器匹配。如果该模式被匹配，
/// 该路由的组件将被渲染。
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    // 临时简化路由，未来将扩展为：
    // - / -> Login 页面
    // - /dashboard -> 主监控面板
    #[route("/")]
    Home {},
}

// 我们可以使用 `asset!` 宏在 dioxus 中导入资源。该宏接受相对于 crate 根目录的资源路径。
// 该宏返回一个 `Asset` 类型，在浏览器中显示为资源路径，或在桌面应用中显示为本地路径。
const FAVICON: Asset = asset!("/assets/favicon.ico");
// asset 宏还会压缩某些资源（如 CSS 和 JS），以减小打包体积
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");
const FONT_MONO: Asset = asset!("/assets/fonts/LXGWWenKaiMono-Regular.woff2");

fn main() {
    // 初始化日志系统
    dioxus_logger::init(dioxus_logger::tracing::Level::INFO).expect("Failed to init logger");

    // `launch` 函数是 dioxus 应用的主入口点。它接受一个组件并使用你启用的平台特性进行渲染
    #[cfg(feature = "desktop")]
    {
        tracing::info!("Starting RemoteHelper Desktop Application");

        let mut cfg = dioxus::desktop::Config::new().with_window(
            dioxus::desktop::WindowBuilder::new()
                .with_title("RemoteHelper - 远程系统监控")
                .with_resizable(true)
                .with_inner_size(dioxus::desktop::LogicalSize::new(1200, 800))
                .with_min_inner_size(dioxus::desktop::LogicalSize::new(800, 600)),
        );

        // 仅在Windows上移除菜单栏
        #[cfg(target_os = "windows")]
        {
            cfg = cfg.with_menu(None);
        }

        dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
    }

    #[cfg(not(feature = "desktop"))]
    {
        dioxus::launch(App);
    }
}

/// App 是我们应用的主组件。组件是 dioxus 应用的构建块。每个组件都是一个函数，
/// 接受一些 props 并返回一个 Element。在这种情况下，App 不接受 props，因为它是我们应用的根组件。
///
/// 组件应该用 `#[component]` 注解，以支持 props、更好的错误消息和自动补全
#[component]
fn App() -> Element {
    // 符合官方推荐：在根组件使用 use_signal 创建响应式状态，然后通过 Context 提供
    use_context_provider(|| use_signal(AuthData::default));
    use_context_provider(|| use_signal(SystemData::default));
    use_context_provider(|| use_signal(ServicesData::default));

    // `rsx!` 宏让我们可以在 rust 中定义 HTML。它会展开为一个包含所有 HTML 的 Element。
    rsx! {
        // 除了元素和文本（稍后我们会看到），rsx 还可以包含其他组件。在这种情况下，
        // 我们使用 `document::Link` 组件将 favicon 和主 CSS 文件的链接添加到应用的 head 中。
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }

        // 设置字体
        document::Style {
            r#"
            @font-face {{
                font-family: 'LXGWWenKaiMono';
                src: url('{FONT_MONO}') format('woff2');
                font-weight: normal;
                font-style: normal;
            }}
            "#
        }

        // router 组件渲染我们上面定义的路由枚举。它将处理 URL 的同步，并渲染
        // 活动路由的布局和组件。
        Router::<Route> {}
    }
}
