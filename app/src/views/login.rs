//! 登录视图
//!
//! 独立的登录页面

use crate::components::Login as LoginForm;
use dioxus::prelude::*;

#[component]
pub fn LoginView() -> Element {
    rsx! {
        div { class: "login-page", LoginForm {} }
    }
}
