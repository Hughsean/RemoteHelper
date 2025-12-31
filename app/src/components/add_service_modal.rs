use dioxus::prelude::*;

#[component]
pub fn AddServiceModal(
    on_close: EventHandler<()>,
    on_add: EventHandler<(String, String, Vec<String>)>,
) -> Element {
    let mut description = use_signal(|| String::new());
    let mut exe_path = use_signal(|| String::new());
    let mut args = use_signal(|| String::new());

    let handle_submit = move |evt: FormEvent| {
        evt.stop_propagation();
        evt.prevent_default();
        let args_vec: Vec<String> = args().split_whitespace().map(|s| s.to_string()).collect();
        on_add.call((description(), exe_path(), args_vec));
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal-box",
                div { class: "modal-header",
                    h3 { class: "modal-title", "添加自定义服务" }
                    button {
                        class: "btn-close",
                        onclick: move |_| on_close.call(()),
                        svg {
                            class: "icon-md",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M6 18L18 6M6 6l12 12",
                            }
                        }
                    }
                }
                form { onsubmit: handle_submit, class: "modal-form",
                    div { class: "form-group",
                        label { class: "form-label", "描述" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{description}",
                            oninput: move |e| description.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "可执行文件路径" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{exe_path}",
                            oninput: move |e| exe_path.set(e.value()),
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "参数 (空格分隔)" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{args}",
                            oninput: move |e| args.set(e.value()),
                        }
                    }
                    div { class: "modal-actions",
                        button {
                            r#type: "button",
                            class: "btn-cancel",
                            onclick: move |_| on_close.call(()),
                            "取消"
                        }
                        button { r#type: "submit", class: "btn-primary", "添加服务" }
                    }
                }
            }
        }
    }
}
