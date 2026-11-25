//! 跨页面共用的界面构件：面板、字段、提示文字与导航按钮。

use common::Response;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants},
    v_flex,
};
use gpui_kit::*;

/// 顶部导航标签：激活时在下方画一条强调色短线。
pub(crate) fn page_tab(active: bool, button: Button, cx: &App) -> Div {
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
                .bg(cx.theme().primary)
        }))
}

/// 导航按钮配色：激活时用强调色，否则是幽灵按钮。
pub(crate) fn nav_variant(active: bool, cx: &App) -> ButtonVariant {
    if active {
        let accent = cx.theme().primary;
        ButtonVariant::Custom(
            ButtonCustomVariant::new(cx)
                .color(accent.opacity(0.14))
                .foreground(accent)
                .hover(accent.opacity(0.24))
                .active(accent.opacity(0.30)),
        )
    } else {
        ButtonVariant::Ghost
    }
}

pub(crate) fn brand_mark(cx: &App) -> Div {
    let accent = cx.theme().primary;
    div()
        .size(px(44.))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .bg(accent.opacity(0.14))
        .text_color(accent)
        .child(Icon::from(IconName::Activity).size(px(20.)))
}

pub(crate) fn small_button(
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

pub(crate) fn panel(cx: &App) -> Div {
    v_flex()
        .min_w_0()
        .gap_3()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
}

pub(crate) fn field(label: &'static str, element: impl IntoElement) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_sm().font_medium().child(label))
        .child(element)
}

pub(crate) fn message(text: impl Into<SharedString>, error: bool, cx: &App) -> Div {
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

pub(crate) fn request_error(response: Response) -> String {
    match response {
        Response::Error(error) => error,
        _ => "服务器返回了意外响应".to_string(),
    }
}
