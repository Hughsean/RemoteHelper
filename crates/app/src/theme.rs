//! 全局主题和原生窗口材质。透明度只用于背景层，文本和图表使用主题前景色。

use crate::preferences::{Appearance, ClientPreferences};
use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::*;

struct Palette {
    background: u32,
    ink: u32,
    muted_ink: u32,
    accent: u32,
    on_accent: u32,
    flyout: u32,
    success: u32,
    danger: u32,
    on_danger: u32,
    metrics: [u32; 3],
}

const DARK: Palette = Palette {
    background: 0x202020,
    ink: 0xf1f2f6,
    muted_ink: 0xa9adbb,
    accent: 0x5ac3f6,
    on_accent: 0x082537,
    flyout: 0x292b31,
    success: 0x63d598,
    danger: 0xff8275,
    on_danger: 0x35120d,
    metrics: [0x53cdef, 0xa698fb, 0xf887ad],
};

const LIGHT: Palette = Palette {
    background: 0xf3f3f4,
    ink: 0x202127,
    muted_ink: 0x575c69,
    accent: 0x075fae,
    on_accent: 0xffffff,
    flyout: 0xfafafd,
    success: 0x167746,
    danger: 0xb82621,
    on_danger: 0xffffff,
    metrics: [0x08769f, 0x6248d2, 0xb82f63],
};

pub(crate) fn install_window_surface(cx: &mut App) {
    gpui_kit::base::Root::register_plugin(cx, |_, _| WindowSurface);
}

struct WindowSurface;

impl Render for WindowSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl gpui_kit::base::RootPlugin for WindowSurface {
    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, cx: &mut App) {
        // GPUI Kit 默认在页面下面铺一层不透明底色；在根表面替换它才能露出磨砂。
        surface
            .style()
            .refine(&StyleRefinement::default().bg(window_tint(cx)));
    }
}

/// 新窗口与系统深浅模式同步，打开后也响应系统外观变化。
pub(crate) fn bind_window(window: &mut Window, cx: &mut App) {
    #[cfg(target_os = "windows")]
    apply_desktop_backdrop(window, window_background(cx));
    apply_window_theme(window, cx);
    window
        .observe_window_appearance(apply_window_theme)
        .detach();
}

pub(crate) fn apply_window_theme(window: &mut Window, cx: &mut App) {
    let mode = match ClientPreferences::get(cx).appearance {
        Appearance::System => ThemeMode::from(window.appearance()),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };
    apply(mode, cx);
}

pub(crate) fn apply_window_material(window: &Window, cx: &mut App) {
    let appearance = window_background(cx);
    window.set_background_appearance(appearance);
    #[cfg(target_os = "windows")]
    apply_desktop_backdrop(window, appearance);
    cx.refresh_windows();
}

fn apply(mode: ThemeMode, cx: &mut App) {
    if cx.theme().mode != mode {
        Theme::change(mode, None, cx);
    }

    let dark = mode.is_dark();
    let palette = if dark { DARK } else { LIGHT };
    let color = |value| Hsla::from(rgb(value));
    let contrast = if dark { white() } else { black() };
    let transparent = Hsla::transparent_black();
    let ink = color(palette.ink);
    let accent = color(palette.accent);
    let layer = white().opacity(if dark { 0.043 } else { 0.50 });
    let hover = contrast.opacity(if dark { 0.075 } else { 0.055 });
    let pressed = contrast.opacity(if dark { 0.11 } else { 0.085 });
    let stroke = contrast.opacity(if dark { 0.12 } else { 0.11 });

    Theme::update(cx, |theme| {
        theme.notification.placement = Anchor::BottomRight;
        theme.notification.max_items = 4;
        theme.notification.width = px(344.);
        let colors = &mut theme.colors;
        colors.background = color(palette.background);
        colors.foreground = ink;
        colors.muted_foreground = color(palette.muted_ink);
        colors.muted = contrast.opacity(if dark { 0.057 } else { 0.045 });
        colors.group_box = layer;
        colors.group_box_foreground = ink;
        colors.border = stroke;
        colors.input = contrast.opacity(if dark { 0.16 } else { 0.15 });
        colors.ring = accent;
        colors.primary = accent;
        colors.primary_foreground = color(palette.on_accent);
        colors.primary_hover = accent.blend(contrast.opacity(0.10));
        colors.primary_active = accent.blend(contrast.opacity(0.18));
        colors.secondary = white().opacity(if dark { 0.064 } else { 0.67 });
        colors.secondary_foreground = ink;
        colors.secondary_hover = colors.secondary.blend(hover);
        colors.secondary_active = colors.secondary.blend(pressed);
        colors.accent = hover;
        colors.accent_foreground = ink;
        colors.popover = color(palette.flyout).opacity(0.97);
        colors.popover_foreground = ink;
        colors.title_bar = transparent;
        colors.title_bar_border = stroke;
        colors.selection = accent.opacity(0.22);
        colors.list = transparent;
        colors.list_even = transparent;
        colors.list_head = layer;
        colors.list_hover = hover;
        colors.list_active = accent.opacity(0.14);
        colors.list_active_border = accent.opacity(0.35);
        colors.scrollbar = transparent;
        colors.scrollbar_thumb = contrast.opacity(0.20);
        colors.scrollbar_thumb_hover = contrast.opacity(0.35);
        colors.link = accent;
        colors.link_hover = colors.primary_hover;
        colors.link_active = colors.primary_active;
        colors.success = color(palette.success);
        colors.success_foreground = color(if dark { 0x09261a } else { 0xffffff });
        colors.success_hover = colors.success.blend(contrast.opacity(0.10));
        colors.success_active = colors.success.blend(contrast.opacity(0.18));
        colors.danger = color(palette.danger);
        colors.danger_foreground = color(palette.on_danger);
        colors.danger_hover = colors.danger.blend(contrast.opacity(0.10));
        colors.danger_active = colors.danger.blend(contrast.opacity(0.18));
        [colors.chart_1, colors.chart_2, colors.chart_3] = palette.metrics.map(color);
        colors.chart_grid = contrast.opacity(0.09);
    });
}

/// 底色着色后仍能露出原生材质；纯色窗口直接使用主题底色。
fn window_tint(cx: &App) -> Hsla {
    let theme = cx.theme();
    let opacity = if window_background(cx) == WindowBackgroundAppearance::Opaque {
        1.
    } else if theme.is_dark() {
        0.26
    } else {
        0.39
    };
    theme.background.opacity(opacity)
}

pub(crate) fn window_background(cx: &App) -> WindowBackgroundAppearance {
    if ClientPreferences::get(cx).frosted {
        native_background()
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

pub(crate) fn supports_frosted() -> bool {
    native_background() != WindowBackgroundAppearance::Opaque
}

#[cfg(target_os = "windows")]
fn native_background() -> WindowBackgroundAppearance {
    use std::{mem::size_of, sync::OnceLock};
    use windows::{
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
    };

    static MATERIAL: OnceLock<WindowBackgroundAppearance> = OnceLock::new();
    *MATERIAL.get_or_init(|| {
        if std::env::var("GPUI_DISABLE_DIRECT_COMPOSITION")
            .is_ok_and(|value| value == "1" || value == "true")
        {
            return WindowBackgroundAppearance::Opaque;
        }

        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // RtlGetVersion 写入按 dwOSVersionInfoSize 初始化的结构。
        if !unsafe { RtlGetVersion(&mut version) }.is_ok() {
            return WindowBackgroundAppearance::Opaque;
        }
        match version.dwBuildNumber {
            // 用 GPUI 的系统背景模式创建透明表面，开窗时切换为 DWM 亚克力。
            22621.. => WindowBackgroundAppearance::MicaBackdrop,
            17763.. => WindowBackgroundAppearance::Blurred,
            _ => WindowBackgroundAppearance::Opaque,
        }
    })
}

#[cfg(target_os = "windows")]
fn apply_desktop_backdrop(window: &Window, appearance: WindowBackgroundAppearance) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
            DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
        },
        UI::Controls::MARGINS,
    };

    if native_background() != WindowBackgroundAppearance::MicaBackdrop {
        return;
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let acrylic = appearance == WindowBackgroundAppearance::MicaBackdrop;
    let material = if acrylic {
        DWMSBT_TRANSIENTWINDOW
    } else {
        DWMSBT_NONE
    };
    let margin = if acrylic { -1 } else { 0 };
    let margins = MARGINS {
        cxLeftWidth: margin,
        cxRightWidth: margin,
        cyTopHeight: margin,
        cyBottomHeight: margin,
    };
    // HWND 来自仍存活的 GPUI 窗口，传入的材质和边距只在调用期间读取。
    unsafe {
        if let Err(error) = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            std::ptr::from_ref(&material).cast(),
            std::mem::size_of_val(&material) as u32,
        ) {
            tracing::debug!(%error, "无法设置 DWM 窗口材质");
            if acrylic {
                window.set_background_appearance(WindowBackgroundAppearance::Blurred);
            }
        } else {
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
    }
}

#[cfg(target_os = "macos")]
fn native_background() -> WindowBackgroundAppearance {
    WindowBackgroundAppearance::Blurred
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn native_background() -> WindowBackgroundAppearance {
    WindowBackgroundAppearance::Opaque
}
