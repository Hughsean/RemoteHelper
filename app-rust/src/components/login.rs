use dioxus::prelude::*;

#[component]
pub fn Login(on_login: EventHandler<(String, String)>) -> Element {
    let mut passphrase = use_signal(|| String::new());
    let mut address = use_signal(|| "frp-try.com:53460".to_string());
    let error = use_signal(|| Option::<String>::None);

    let handle_submit = move |evt: FormEvent| {
        evt.stop_propagation();
        evt.prevent_default();
        on_login.call((passphrase(), address()));
    };

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-sm",
            div { class: "bg-slate-900 p-8 rounded-2xl shadow-2xl border border-slate-800 w-full max-w-md",
                div { class: "text-center mb-8",
                    div { class: "w-16 h-16 bg-indigo-500 rounded-2xl mx-auto flex items-center justify-center shadow-lg shadow-indigo-500/20 mb-4",
                        svg {
                            class: "w-8 h-8 text-white",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z",
                            }
                        }
                    }
                    h2 { class: "text-2xl font-bold text-white", "Authentication Required" }
                    p { class: "text-slate-400 mt-2", "Please enter your credentials to continue" }
                }

                form { onsubmit: handle_submit, class: "space-y-4",
                    div {
                        label { class: "block text-sm font-medium text-slate-400 mb-1",
                            "Server Address"
                        }
                        input {
                            r#type: "text",
                            class: "w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-white focus:ring-2 focus:ring-indigo-500 focus:border-transparent outline-none transition-all",
                            value: "{address}",
                            oninput: move |e| address.set(e.value()),
                        }
                    }
                    div {
                        label { class: "block text-sm font-medium text-slate-400 mb-1",
                            "Passphrase"
                        }
                        input {
                            r#type: "password",
                            class: "w-full bg-slate-800 border border-slate-700 rounded-lg px-4 py-2.5 text-white focus:ring-2 focus:ring-indigo-500 focus:border-transparent outline-none transition-all",
                            value: "{passphrase}",
                            oninput: move |e| passphrase.set(e.value()),
                        }
                    }
                    if let Some(err) = error() {
                        div { class: "text-red-500 text-sm text-center", "{err}" }
                    }
                    button {
                        r#type: "submit",
                        class: "w-full bg-indigo-600 hover:bg-indigo-500 text-white font-medium py-2.5 rounded-lg transition-colors shadow-lg shadow-indigo-500/20",
                        "Connect"
                    }
                }
            }
        }
    }
}
