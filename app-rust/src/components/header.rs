use dioxus::prelude::*;

#[component]
pub fn Header(on_refresh_change: EventHandler<u64>) -> Element {
    rsx! {
        header { class: "bg-slate-900/80 backdrop-blur-md border-b border-slate-800/50 px-6 py-4 flex justify-between items-center sticky top-0 z-10 shadow-sm",
            div { class: "flex items-center gap-3",
                div { class: "w-8 h-8 bg-indigo-500 rounded-lg flex items-center justify-center shadow-lg shadow-indigo-500/20 transition-transform hover:scale-105 hover:rotate-3",
                    svg {
                        class: "w-5 h-5 text-white",
                        fill: "none",
                        stroke: "currentColor",
                        view_box: "0 0 24 24",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            stroke_width: "2",
                            d: "M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10",
                        }
                    }
                }
                h1 { class: "text-xl font-bold text-white tracking-tight", "服务监控(Hughsean)" }
            }
            div { class: "flex items-center gap-3",
                div { class: "relative bg-slate-800/50 rounded-lg border border-slate-700 hover:border-slate-600 transition-all duration-300 group w-40 hover:bg-slate-800",
                    div { class: "flex items-center justify-between px-3 py-1.5 pointer-events-none h-full",
                        span { class: "text-xs text-slate-400 group-hover:text-slate-300 transition-colors",
                            "刷新间隔"
                        }
                        div { class: "flex items-center gap-1",
                            // TODO: Display current interval
                            span { class: "text-sm text-white font-medium", "1s" }
                            svg {
                                class: "w-4 h-4 text-slate-500 group-hover:text-slate-300 transition-colors",
                                fill: "none",
                                stroke: "currentColor",
                                view_box: "0 0 24 24",
                                path {
                                    stroke_linecap: "round",
                                    stroke_linejoin: "round",
                                    stroke_width: "2",
                                    d: "M19 9l-7 7-7-7",
                                }
                            }
                        }
                    }
                    select {
                        class: "absolute inset-0 w-full h-full opacity-0 cursor-pointer [&>option]:bg-slate-800 [&>option]:text-slate-200",
                        onchange: move |evt| {
                            if let Ok(val) = evt.value().parse::<u64>() {
                                on_refresh_change.call(val);
                            }
                        },
                        option { value: "100", "0.1s" }
                        option { value: "500", "0.5s" }
                        option { value: "1000", selected: true, "1s" }
                        option { value: "3000", "3s" }
                        option { value: "10000", "5s" }
                        option { value: "0", "暂停" }
                    }
                }
            }
        }
    }
}
