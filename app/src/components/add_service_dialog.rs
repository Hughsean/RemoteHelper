use common::{PathItem, Request, Response};
use dioxus::document::eval;
use dioxus::prelude::*;

const DIALOG_CSS: Asset = asset!("/assets/styling/add_service_dialog.css");

#[component]
pub fn AddServiceDialog(on_close: EventHandler<()>, on_success: EventHandler<()>) -> Element {
    let mut description = use_signal(|| String::new());
    let mut exe_path = use_signal(|| String::new());
    let mut args = use_signal(|| String::new());
    let mut path_suggestions = use_signal(|| Vec::<PathItem>::new());
    let mut show_suggestions = use_signal(|| false);
    let mut is_submitting = use_signal(|| false);
    let mut selected_index = use_signal(|| 0_i32);

    // 监听选中索引变化，自动滚动
    use_effect(move || {
        if show_suggestions() && !path_suggestions().is_empty() {

            spawn(async move {
                let script = format!(
                    r#"
                    setTimeout(() => {{
                        const item = document.querySelector('.suggestion-item.selected');
                        if (item) {{
                            item.scrollIntoView({{ block: 'nearest', behavior: 'smooth' }});
                        }}
                    }}, 10);
                    "#
                );
                let _ = eval(&script);
            });
        }
    });

    // 查询路径建议
    let query_path = move |path: String| {
        spawn(async move {
            if path.is_empty() {
                path_suggestions.set(vec![]);
                show_suggestions.set(false);
                selected_index.set(0);
                return;
            }

            match client::send_request(Request::QueryPath { path: path.clone() }).await {
                Ok(Response::PathSuggestions(items)) => {
                    path_suggestions.set(items);
                    show_suggestions.set(true);
                    selected_index.set(0);
                }
                _ => {
                    path_suggestions.set(vec![]);
                    show_suggestions.set(false);
                    selected_index.set(0);
                }
            }
        });
    };

    // 选择建议项
    let mut select_suggestion = move |index: usize| {
        if let Some(item) = path_suggestions().get(index) {
            exe_path.set(item.path.clone());
            show_suggestions.set(false);
            selected_index.set(0);
        }
    };

    // 提交添加服务
    let submit = move |_| {
        if description().trim().is_empty() || exe_path().trim().is_empty() {
            return;
        }

        is_submitting.set(true);
        let desc = description().clone();
        let path = exe_path().clone();
        let args_vec: Vec<String> = args().split_whitespace().map(|s| s.to_string()).collect();

        spawn(async move {
            match client::send_request(Request::AddService {
                description: desc,
                exe_path: path,
                args: args_vec,
            })
            .await
            {
                Ok(Response::ServiceAdded(_)) => {
                    tracing::info!("服务添加成功");
                    on_success.call(());
                }
                Ok(Response::Error(e)) => {
                    tracing::error!("添加服务失败: {}", e);
                    is_submitting.set(false);
                }
                _ => {
                    tracing::error!("添加服务失败: 意外响应");
                    is_submitting.set(false);
                }
            }
        });
    };

    rsx! {
        document::Link { rel: "stylesheet", href: DIALOG_CSS }
        div { class: "dialog-overlay", onclick: move |_| on_close.call(()),
            div {
                class: "dialog-content",
                onclick: move |e| e.stop_propagation(),

                div { class: "dialog-header",
                    h2 { "添加自定义服务" }
                    button {
                        class: "close-button",
                        onclick: move |_| on_close.call(()),
                        "×"
                    }
                }

                div { class: "dialog-body",
                    // 描述输入
                    div { class: "form-group",
                        label { "描述" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            placeholder: "输入服务描述...",
                            value: "{description}",
                            oninput: move |e| description.set(e.value().clone()),
                        }
                    }

                    // 可执行文件路径
                    div { class: "form-group",
                        label { "可执行文件路径" }
                        div { class: "path-input-wrapper",
                            input {
                                r#type: "text",
                                class: "form-input",
                                placeholder: "输入或选择可执行文件路径...",
                                value: "{exe_path}",
                                oninput: move |e| {
                                    let val = e.value().clone();
                                    exe_path.set(val.clone());
                                    query_path(val);
                                },
                                onfocus: move |_| {
                                    if !path_suggestions().is_empty() {
                                        show_suggestions.set(true);
                                    }
                                },
                                onkeydown: move |e| {
                                    if !show_suggestions() || path_suggestions().is_empty() {
                                        return;
                                    }

                                    let key = e.key().to_string();
                                    let suggestions_count = path_suggestions().len() as i32;

                                    match key.as_str() {
                                        "ArrowDown" => {
                                            e.prevent_default();
                                            let new_index = if selected_index() >= suggestions_count - 1 {
                                                0 // 循环到顶部
                                            } else {
                                                selected_index() + 1
                                            };
                                            selected_index.set(new_index);
                                        }
                                        "ArrowUp" => {
                                            e.prevent_default();
                                            let new_index = if selected_index() <= 0 {
                                                suggestions_count - 1 // 循环到底部
                                            } else {
                                                selected_index() - 1
                                            };
                                            selected_index.set(new_index);
                                        }
                                        "Enter" => {
                                            e.prevent_default();
                                            select_suggestion(selected_index() as usize);
                                        }
                                        "Escape" => {
                                            e.prevent_default();
                                            show_suggestions.set(false);
                                            selected_index.set(0);
                                        }
                                        _ => {}
                                    }
                                },
                            }

                            // 路径建议列表
                            if show_suggestions() && !path_suggestions().is_empty() {
                                div { class: "path-suggestions",
                                    for (idx , item) in path_suggestions().iter().enumerate() {
                                        div {
                                            class: if idx == selected_index() as usize { "suggestion-item selected" } else { "suggestion-item" },
                                            onclick: move |_| {
                                                select_suggestion(idx);
                                            },

                                            if item.is_dir {
                                                span { class: "suggestion-icon", "📁 " }
                                            } else if item.is_executable {
                                                span { class: "suggestion-icon", "⚙ " }
                                            } else {
                                                span { class: "suggestion-icon", "📄 " }
                                            }
                                            span { class: "suggestion-name", "{item.path}" }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // 命令行参数
                    div { class: "form-group",
                        label { "命令行参数（可选）" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            placeholder: "输入启动参数，用空格分隔...",
                            value: "{args}",
                            oninput: move |e| args.set(e.value().clone()),
                        }
                    }
                
                }

                div { class: "dialog-footer",
                    button {
                        class: "btn-cancel",
                        onclick: move |_| on_close.call(()),
                        disabled: is_submitting(),
                        "取消"
                    }
                    button {
                        class: "btn-submit",
                        onclick: submit,
                        disabled: is_submitting() || description().trim().is_empty() || exe_path().trim().is_empty(),
                        if is_submitting() {
                            "添加中..."
                        } else {
                            "添加服务"
                        }
                    }
                }
            }
        }
    }
}
