//! GPUI 装配层：创建应用、注册窗口重开处理、打开主窗口。

use crate::assets::AppAssets;
use crate::preferences::ClientPreferences;
use crate::shell::RemoteHelper;
use crate::theme;
use gpui_kit::*;
use tokio::runtime::Handle;

/// 启动 GPUI 事件循环。`runtime` 由调用方持有，必须活得比事件循环更久。
pub(crate) fn run(runtime: Handle) {
    let application = gpui_kit::application().with_assets(AppAssets);
    let reopen_runtime = runtime.clone();
    // macOS 上关闭最后一个窗口后进程仍存活，必须自己响应 Dock 图标的重开请求，
    // 否则图标会一直留在 Dock 里却打不开任何窗口。
    application.on_reopen(move |cx| {
        if !cx.windows().is_empty() {
            cx.activate(true);
            return;
        }
        match open_main_window(reopen_runtime.clone(), cx) {
            Ok(()) => cx.activate(true),
            Err(error) => tracing::error!(%error, "无法重新打开 RemoteHelper 窗口"),
        }
    });
    application.run(move |cx| {
        gpui_kit::init(cx);
        ClientPreferences::load().install(cx);
        theme::install_window_surface(cx);
        // 开发用：只渲染趋势图并截图，不登录。
        #[cfg(debug_assertions)]
        if std::env::var_os("REMOTEHELPER_PREVIEW").is_some() {
            match crate::preview::open(cx) {
                Ok(()) => {}
                Err(error) => {
                    tracing::error!(%error, "无法打开预览窗口");
                    cx.quit();
                }
            }
            return;
        }
        match open_main_window(runtime.clone(), cx) {
            Ok(()) => cx.activate(true),
            Err(error) => {
                tracing::error!(%error, "无法打开 RemoteHelper 窗口");
                cx.quit();
            }
        }
    });
}

/// 打开主窗口。启动与 Dock 图标重开共用，界面状态由 [`RemoteHelper`] 自己还原。
fn open_main_window(runtime: Handle, cx: &mut App) -> gpui_kit::Result<()> {
    gpui_kit::open_window(
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
            window_background: theme::window_background(cx),
            window_min_size: Some(size(px(780.), px(580.))),
            ..Default::default()
        },
        cx,
        |window, cx| {
            theme::bind_window(window, cx);
            cx.new(|cx| RemoteHelper::new(runtime.clone(), window, cx))
        },
    )?;
    Ok(())
}
