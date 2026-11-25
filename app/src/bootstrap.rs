//! GPUI 装配层：创建应用并打开主窗口。

use crate::assets::AppAssets;
use crate::shell::RemoteHelper;
use crate::theme;
use gpui_kit::*;
use tokio::runtime::Handle;

/// 启动 GPUI 事件循环。`runtime` 由调用方持有，必须活得比事件循环更久。
pub(crate) fn run(runtime: Handle) {
    let application = gpui_kit::application().with_assets(AppAssets);
    application.run(move |cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        match open_main_window(runtime.clone(), cx) {
            Ok(()) => cx.activate(true),
            Err(error) => {
                tracing::error!(%error, "无法打开 RemoteHelper 窗口");
                cx.quit();
            }
        }
    });
}

/// 打开主窗口。
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
            #[cfg(target_os = "windows")]
            window_background: WindowBackgroundAppearance::MicaBackdrop,
            window_min_size: Some(size(px(780.), px(580.))),
            ..Default::default()
        },
        cx,
        |window, cx| cx.new(|cx| RemoteHelper::new(runtime.clone(), window, cx)),
    )?;
    Ok(())
}
