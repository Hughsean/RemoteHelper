//! 顶部导航栏组件

use crate::state::use_system_state;
use dioxus::prelude::*;

#[component]
pub fn Header() -> Element {
    let mut system = use_system_state();
    let mut is_open = use_signal(|| false);

    let refresh_interval = system.read().refresh_interval;

    let display_text = match refresh_interval {
        0 => "暂停",
        100 => "0.1s",
        500 => "0.5s",
        1000 => "1s",
        3000 => "3s",
        5000 => "5s",
        10000 => "10s",
        _ => "自定义",
    };

    // 计算运行时间（基于最早和最新的数据点）
    let uptime = {
        let data = system.read();
        if let (Some(oldest), Some(newest)) = (data.history.front(), data.history.back()) {
            (newest.0 - oldest.0) / 1000
        } else {
            0
        }
    };

    let format_uptime = |seconds: u64| -> String {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        let minutes = (seconds % 3600) / 60;
        let secs = seconds % 60;

        if days > 0 {
            format!("{days:02}d {hours:02}h {minutes:02}m {secs:02}s")
        } else if hours > 0 {
            format!("{hours:02}h {minutes:02}m {secs:02}s")
        } else if minutes > 0 {
            format!("{minutes:02}m {secs:02}s")
        } else {
            format!("{secs:02}s")
        }
    };

    let options = vec![
        (100, "0.1s"),
        (500, "0.5s"),
        (1000, "1s"),
        (3000, "3s"),
        (5000, "5s"),
        (10000, "10s"),
        (0, "暂停"),
    ];

    rsx! {
        header { class: "app-header",
            div { class: "header-left",
                div { class: "logo-box",
                    // Logo placeholder - 暂时使用 SVG 图标
                    svg {
                        class: "icon-md",
                        fill: "currentColor",
                        view_box: "0 0 24 24",
                        path { d: "M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" }
                    }
                }
                div { class: "title-box",
                    h1 { class: "header-title", "服务监控" }
                    p { class: "header-subtitle", "Powered by Hughsean" }
                }
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
                div {
                    class: "refresh-control",
                    onclick: move |_| is_open.set(!is_open()),
                    div { class: "refresh-display",
                        span { class: "refresh-label", "刷新间隔" }
                        div { class: "refresh-value-box",
                            span { class: "refresh-value", "{display_text}" }
                            svg {
                                class: "icon-xs",
                                style: "width: 1rem; height: 1rem; color: #94a3b8; transition: transform 0.2s;",
                                transform: if is_open() { "rotate(180)" } else { "rotate(0)" },
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

                    if is_open() {
                        div { class: "refresh-dropdown-menu",
                            for (val , label) in options {
                                div {
                                    class: if refresh_interval == val { "refresh-option active" } else { "refresh-option" },
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        system.write().set_refresh_interval(val);
                                        is_open.set(false);
                                    },
                                    span { "{label}" }
                                    if refresh_interval == val {
                                        svg {
                                            class: "icon-xs",
                                            style: "width: 1rem; height: 1rem;",
                                            fill: "none",
                                            stroke: "currentColor",
                                            view_box: "0 0 24 24",
                                            path {
                                                stroke_linecap: "round",
                                                stroke_linejoin: "round",
                                                stroke_width: "2",
                                                d: "M5 13l4 4L19 7",
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                button {
                    class: "header-btn ml-4",
                    title: "登出",
                    onclick: move |_| {
                        use crate::state::use_auth_state;
                        let mut auth = use_auth_state();
                        auth.write().logout();
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
