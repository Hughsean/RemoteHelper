//! 跨页面共用的界面构件：面板、字段、提示文字与导航按钮。

use common::Response;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::{
    ActiveTheme, Icon,
    button::{Button, ButtonCustomVariant, ButtonVariant},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::*;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionStatus {
    Connecting,
    Online,
    Offline,
}

impl ConnectionStatus {
    pub(crate) fn set(&mut self, status: Self, cx: &mut Context<Self>) {
        if *self != status {
            *self = status;
            cx.notify();
        }
    }
}

impl Render for ConnectionStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (label, color) = match self {
            Self::Connecting => ("连接中", theme.warning),
            Self::Online => ("在线", theme.success),
            Self::Offline => ("已断开", theme.danger),
        };
        div()
            .id("connection-status")
            .role(gpui_kit::accesskit::Role::Status)
            .aria_label(label)
            .flex()
            .size(px(28.))
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(color.opacity(0.15))
                    .child(div().size(px(8.)).rounded_full().bg(color)),
            )
            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
    }
}

pub(crate) fn trend_title() -> impl IntoElement {
    div()
        .id("trend-heading")
        .text_size(px(16.))
        .font_semibold()
        .child("资源使用率趋势")
        .tooltip_show_delay(Duration::from_millis(950))
        .tooltip(|window, cx| {
            Tooltip::element(|_, _| {
                v_flex()
                    .gap_1()
                    .text_xs()
                    .child("滚轮缩放，Shift + 滚轮平移时间轴")
                    .child("拖动底部选框查看历史，双侧手柄调整范围")
                    .child("左击图表定格，右击或点右箭头回到实时")
                    .child("双击底部滑轨恢复默认窗口")
            })
            .build(window, cx)
        })
}

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

pub(crate) fn request_error(response: Response) -> String {
    match response {
        Response::Error(error) => error,
        _ => "服务器返回了意外响应".to_string(),
    }
}
