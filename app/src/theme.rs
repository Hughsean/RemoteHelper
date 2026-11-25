//! 全局配色：主题初始化，以及各页面共用的取色函数。

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

/// 写入深色主题与 RemoteHelper 的配色。启动时调用一次。
pub(crate) fn init(cx: &mut App) {
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
}

/// 窗口底色：Windows 上叠加半透明色以露出 Mica 背衬。
pub(crate) fn window_tint() -> Hsla {
    #[cfg(target_os = "windows")]
    {
        workspace_background().opacity(0.76)
    }
    #[cfg(not(target_os = "windows"))]
    {
        workspace_background()
    }
}

pub(crate) fn workspace_background() -> Hsla {
    rgb(0x1b2222).into()
}

pub(crate) fn surface() -> Hsla {
    rgb(0x283031).into()
}

pub(crate) fn accent() -> Hsla {
    rgb(0x70dbc0).into()
}
