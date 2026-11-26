//! macOS 应用菜单与窗口动作。
//!
//! 应用菜单承载设置、隐藏和退出等命令；`Command+W` 等窗口快捷键没有对应菜单项，
//! 由 GPUI 全局键绑定直接分发。菜单和按键动作都可能在窗口更新期间执行，窗口操作
//! 必须推迟到本轮更新结束后，否则再次借用同一个窗口会失败。

use crate::shell::MainView;
use gpui_kit::*;
use tokio::runtime::Handle;

actions!(
    remote_helper,
    [
        OpenSettings,
        Quit,
        CloseWindow,
        MinimizeWindow,
        ToggleFullscreen,
        HideApp,
        HideOtherApps,
        ShowAllApps
    ]
);

/// 在打开窗口前安装菜单与快捷键。只在 macOS 上调用。
pub(crate) fn install(runtime: Handle, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
    ]);

    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &CloseWindow, cx| {
        update_active_window(cx, |window, _| window.remove_window());
    });
    cx.on_action(|_: &MinimizeWindow, cx| {
        update_active_window(cx, |window, _| window.minimize_window());
    });
    cx.on_action(|_: &ToggleFullscreen, cx| {
        update_active_window(cx, |window, _| window.toggle_fullscreen());
    });
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(move |_: &OpenSettings, cx| open_settings(runtime.clone(), cx));

    cx.set_menus([Menu::new("RemoteHelper").items([
        MenuItem::action("设置…", OpenSettings),
        MenuItem::separator(),
        MenuItem::action("隐藏 RemoteHelper", HideApp),
        MenuItem::action("隐藏其他", HideOtherApps),
        MenuItem::action("显示全部", ShowAllApps),
        MenuItem::separator(),
        MenuItem::action("退出 RemoteHelper", Quit),
    ])]);
}

fn open_settings(runtime: Handle, cx: &mut App) {
    let Some(view) = cx
        .try_global::<MainView>()
        .and_then(|main| main.0.upgrade())
    else {
        // 没有主窗口时重开一个，随后再切到设置页。
        match crate::bootstrap::open_main_window(runtime, cx) {
            Ok(window) => {
                let view = cx
                    .try_global::<MainView>()
                    .and_then(|main| main.0.upgrade());
                if let Some(view) = view {
                    reveal_settings(window, view, cx);
                }
            }
            Err(error) => tracing::error!(%error, "无法重新打开 RemoteHelper 窗口"),
        }
        return;
    };
    let Some(window) = cx.active_window().or_else(|| cx.windows().last().copied()) else {
        return;
    };
    reveal_settings(window, view, cx);
}

fn reveal_settings(
    window: AnyWindowHandle,
    view: Entity<crate::shell::RemoteHelper>,
    cx: &mut App,
) {
    cx.defer(move |cx| {
        let result = window.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.open_settings(window, cx))
        });
        if let Err(error) = result {
            tracing::warn!(%error, "无法打开设置页");
        }
    });
}

fn update_active_window(cx: &mut App, update: impl FnOnce(&mut Window, &mut App) + 'static) {
    let Some(window) = cx.active_window().or_else(|| cx.windows().last().copied()) else {
        return;
    };
    cx.defer(move |cx| {
        if let Err(error) = window.update(cx, |_, window, cx| update(window, cx)) {
            tracing::warn!(%error, "无法更新活动窗口");
        }
    });
}

#[cfg(test)]
mod tests {
    use gpui_kit::Keystroke;

    #[test]
    fn shortcut_strings_parse() {
        for source in [
            "cmd-,",
            "cmd-q",
            "cmd-w",
            "cmd-m",
            "ctrl-cmd-f",
            "cmd-h",
            "cmd-alt-h",
        ] {
            Keystroke::parse(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        }
    }
}
