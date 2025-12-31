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
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-sm",
            div { class: "bg-slate-900 p-6 rounded-xl shadow-2xl border border-slate-800 w-full max-w-lg",
                div { class: "flex justify-between items-center mb-6",
                    h3 { class: "text-xl font-bold text-white", "Add Custom Service" }
                    button {
                        class: "text-slate-400 hover:text-white transition-colors",
                        onclick: move |_| on_close.call(()),
                        svg {
                            class: "w-6 h-6",
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
                form { onsubmit: handle_submit, class: "space-y-4",
                    div {
                        label { class: "block text-sm font-medium text-slate-400 mb-1",
                            "Description"
                        }
                        input {
                            r#type: "text",
                            class: "w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-indigo-500 outline-none",
                            value: "{description}",
                            oninput: move |e| description.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-slate-400 mb-1",
                            "Executable Path"
                        }
                        input {
                            r#type: "text",
                            class: "w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-indigo-500 outline-none",
                            value: "{exe_path}",
                            oninput: move |e| exe_path.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-slate-400 mb-1",
                            "Arguments (space separated)"
                        }
                        input {
                            r#type: "text",
                            class: "w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2 text-white focus:ring-2 focus:ring-indigo-500 outline-none",
                            value: "{args}",
                            oninput: move |e| args.set(e.value()),
                        }
                    }
                    div { class: "flex justify-end gap-3 mt-6",
                        button {
                            r#type: "button",
                            class: "px-4 py-2 text-slate-400 hover:text-white transition-colors",
                            onclick: move |_| on_close.call(()),
                            "Cancel"
                        }
                        button {
                            r#type: "submit",
                            class: "px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition-colors",
                            "Add Service"
                        }
                    }
                }
            }
        }
    }
}
