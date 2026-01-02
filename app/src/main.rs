// dioxus prelude 包含了大量在 dioxus 应用中常用的项。在任何需要 dioxus 的地方导入它都是个好主意
use dioxus::prelude::*;

use views::{LoginView, Dashboard};

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
    #[route("/")]
    LoginView {},
    #[route("/dashboard")]
    Dashboard {},
}

// 我们可以使用 `asset!` 宏在 dioxus 中导入资源。该宏接受相对于 crate 根目录的资源路径。
// 该宏返回一个 `Asset` 类型，在浏览器中显示为资源路径，或在桌面应用中显示为本地路径。
const FAVICON: Asset = asset!("/assets/favicon.ico");
// 主样式文件 - 包含所有自定义样式，不使用 Tailwind 以避免样式冲突
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");
const FONT_MONO: Asset = asset!("/assets/fonts/LXGWWenKaiMono-Regular.woff2");

/// Windows 平台检查 WebView2 Runtime 是否已安装
#[cfg(all(feature = "desktop", target_os = "windows"))]
fn check_webview2() -> Result<(), String> {
    use std::process::Command;

    dioxus_logger::tracing::info!("正在检查 WebView2 Runtime...");

    let output = Command::new("reg")
        .args(&[
            "query",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "/v",
            "pv"
        ])
        .output()
        .map_err(|e| format!("无法检查 WebView2: {}", e))?;

    if !output.status.success() {
        dioxus_logger::tracing::error!("WebView2 Runtime 未安装或无法访问注册表");
        dioxus_logger::tracing::error!(
            "请访问 https://go.microsoft.com/fwlink/p/?LinkId=2124703 下载安装"
        );
        return Err("WebView2 Runtime 未安装".to_string());
    }

    let version_info = String::from_utf8_lossy(&output.stdout);
    dioxus_logger::tracing::info!("WebView2 检查完成: {}", version_info.trim());

    Ok(())
}

#[cfg(not(all(feature = "desktop", target_os = "windows")))]
fn check_webview2() -> Result<(), String> {
    Ok(())
}

fn main() {
    // 使用 println! 确保能看到输出，即使日志系统失败
    println!("========================================");
    println!("RemoteHelper v{} 启动中...", env!("CARGO_PKG_VERSION"));
    println!("========================================");

    // 初始化日志系统
    println!("[1/4] 正在初始化日志系统...");
    dioxus_logger::init(dioxus_logger::tracing::Level::INFO).expect("Failed to init logger");
    println!("[2/4] 日志系统初始化完成");

    dioxus_logger::tracing::info!("========================================");
    dioxus_logger::tracing::info!("RemoteHelper 启动中...");
    dioxus_logger::tracing::info!("版本: {}", env!("CARGO_PKG_VERSION"));
    dioxus_logger::tracing::info!("========================================");

    // `launch` 函数是 dioxus 应用的主入口点。它接受一个组件并使用你启用的平台特性进行渲染
    #[cfg(feature = "desktop")]
    {
        println!("[3/4] 平台: Desktop (Windows)");
        dioxus_logger::tracing::info!("平台: Desktop (Windows)");
        dioxus_logger::tracing::info!("Starting RemoteHelper Desktop Application");

        // 检查 WebView2 Runtime
        println!("[3/4] 正在检查 WebView2 Runtime...");
        if let Err(e) = check_webview2() {
            dioxus_logger::tracing::error!("WebView2 检查失败: {}", e);
            eprintln!("\n❌ 错误: {}", e);
            eprintln!("请安装 Microsoft Edge WebView2 Runtime");
            eprintln!("下载地址: https://go.microsoft.com/fwlink/p/?LinkId=2124703\n");
            std::process::exit(1);
        }
        println!("[3/4] WebView2 检查通过");

        println!("[4/4] 正在创建窗口配置...");
        dioxus_logger::tracing::info!("正在创建窗口配置...");
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
            dioxus_logger::tracing::info!("移除 Windows 菜单栏");
            cfg = cfg.with_menu(None);
        }

        println!("[4/4] 正在启动 Dioxus 应用...");
        dioxus_logger::tracing::info!("正在启动 Dioxus 应用...");
        dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
        println!("[✓] 应用已正常退出");
        dioxus_logger::tracing::info!("应用已退出");
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
    // 符合官方推荐：在根组件创建响应式状态，然后通过 Context 提供
    // 注意：不能在 hook 内部调用另一个 hook，所以先创建 signal 再提供
    let auth = use_signal(AuthData::default);
    let system = use_signal(SystemData::default);
    let services = use_signal(ServicesData::default);

    use_context_provider(|| auth);
    use_context_provider(|| system);
    use_context_provider(|| services);

    // `rsx!` 宏让我们可以在 rust 中定义 HTML。它会展开为一个包含所有 HTML 的 Element。
    rsx! {
        // 除了元素和文本（稍后我们会看到），rsx 还可以包含其他组件。在这种情况下，
        // 我们使用 `document::Link` 组件将 favicon 和主 CSS 文件的链接添加到应用的 head 中。
        document::Link { rel: "icon", href: FAVICON }
        // 只加载我们的自定义样式，不使用 Tailwind 以避免样式冲突
        document::Link { rel: "stylesheet", href: MAIN_CSS }

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
