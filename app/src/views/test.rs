use dioxus::prelude::*;

#[component]
pub fn Test() -> Element {
    rsx! {
        div { class: "test-page",
            h1 { "测试页面" }
            p { "这是一个测试页面，用于验证功能。" }
            button { onclick: move |_| tracing::info!("测试按钮被点击"), "点击测试" }
        }
    }
}
