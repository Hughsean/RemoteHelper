use dioxus::prelude::*;

/// Home 页面组件，当当前路由为 `[Route::Home]` 时渲染
/// 临时占位组件，将在后续迁移中重构为 Dashboard
#[component]
pub fn Home() -> Element {
    rsx! {
        div { class: "min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100 flex items-center justify-center",
            div { class: "text-center",
                h1 { class: "text-4xl font-bold text-gray-800 mb-4", "RemoteHelper 监控面板" }
                p { class: "text-gray-600 mb-8", "正在迁移中... 即将推出新界面" }
                div { class: "text-sm text-gray-500",
                    "App-D 将提供："
                    ul { class: "mt-2 space-y-1",
                        li { "✓ 系统状态实时监控" }
                        li { "✓ 服务管理" }
                        li { "✓ 趋势图表展示" }
                        li { "✓ WebSocket 连接" }
                    }
                }
            }
        }
    }
}
