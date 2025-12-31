use dioxus::prelude::*;

#[component]
pub fn Header(refresh_interval: u64, uptime: u64, on_refresh_change: EventHandler<u64>) -> Element {
    let display_text = match refresh_interval {
        0 => "暂停",
        100 => "0.1s",
        500 => "0.5s",
        1000 => "1s",
        3000 => "3s",
        10000 => "5s",
        _ => "自定义",
    };

    let format_uptime = |seconds: u64| -> String {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;
        
        if days > 0 {
            format!("{}d {}h {}m {}s", days, hours, minutes, secs)
        } else if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, secs)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, secs)
        } else {
            format!("{}s", secs)
        }
    };

    rsx! {
        header { class: "app-header",
            div { class: "header-left",
                div { class: "logo-box",
                    svg {
                        class: "icon-md",
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
                h1 { class: "header-title", "服务监控" }
                if uptime > 0 {
                    div { class: "uptime-badge",
                        svg {
                            class: "icon-sm",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z",
                            }
                        }
                        span { "服务器已上线: {format_uptime(uptime)}" }
                    }
                }
            }
            div { class: "header-right",
                div { class: "refresh-control",
                    div { class: "refresh-display",
                        span { class: "refresh-label", "刷新间隔" }
                        div { class: "refresh-value-box",
                            span { class: "refresh-value", "{display_text}" }
                        }
                    }
                    select {
                        class: "refresh-select",
                        onchange: move |evt| {
                            if let Ok(val) = evt.value().parse::<u64>() {
                                on_refresh_change.call(val);
                            }
                        },
                        option { value: "100", selected: refresh_interval == 100, "0.1s" }
                        option { value: "500", selected: refresh_interval == 500, "0.5s" }
                        option { value: "1000", selected: refresh_interval == 1000, "1s" }
                        option { value: "3000", selected: refresh_interval == 3000, "3s" }
                        option {
                            value: "10000",
                            selected: refresh_interval == 10000,
                            "5s"
                        }
                        option { value: "0", selected: refresh_interval == 0, "暂停" }
                    }
                }

                button {
                    class: "header-btn ml-4",
                    title: "退出程序",
                    onclick: move |_| {
                        spawn(async move {
                            crate::command::quit_app().await;
                        });
                    },
                    svg {
                        class: "icon-md",
                        fill: "none",
                        stroke: "currentColor",
                        view_box: "0 0 24 24",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            stroke_width: "2",
                            d: "M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1",
                        }
                    }
                }
            }
        }
    }
}
