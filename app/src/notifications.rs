//! 窗口级通知。GPUI 的通知层独立于页面与弹窗布局。

use gpui_kit::component::{WindowExt, notification::Notification};
use gpui_kit::*;

#[derive(Clone, Copy)]
pub(crate) struct Notices(AnyWindowHandle);

impl Notices {
    pub(crate) fn new(window: &Window) -> Self {
        Self(window.window_handle())
    }

    pub(crate) fn show(self, notice: Notification, cx: &mut App) {
        // 页面回调执行时窗口可能正在更新，等当前更新结束后再进入通知层。
        cx.defer(move |cx| {
            let _ = self.0.update(cx, |_, window, cx| {
                window.push_notification(notice, cx);
            });
        });
    }

    pub(crate) fn warning(self, message: impl Into<SharedString>, cx: &mut App) {
        self.show(Notification::warning(message).title("提示"), cx);
    }

    pub(crate) fn error(self, message: impl Into<SharedString>, cx: &mut App) {
        self.show(Notification::error(message).title("操作失败"), cx);
    }

    pub(crate) fn success(self, message: impl Into<SharedString>, cx: &mut App) {
        self.show(Notification::success(message), cx);
    }
}
