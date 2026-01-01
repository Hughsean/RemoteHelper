#![allow(non_snake_case)]

use crate::command;
use crate::components::{
    add_service_modal::AddServiceModal, header::Header, login::Login, service_list::ServiceList,
    system_status::SystemStatusDisplay, trend_chart::TrendChart,
};
// use crate::models::{ServiceInfo, SystemInfo};
use dioxus::prelude::*;
use std::collections::VecDeque;
use std::time::Duration;

static BASE_CSS: Asset = asset!("/assets/css/base.css");
static HEADER_CSS: Asset = asset!("/assets/css/header.css");
static LOGIN_CSS: Asset = asset!("/assets/css/login.css");
static STATUS_CSS: Asset = asset!("/assets/css/status.css");
static SERVICES_CSS: Asset = asset!("/assets/css/services.css");
static CHART_CSS: Asset = asset!("/assets/css/chart.css");
static FONT_MONO: Asset = asset!("/assets/fonts/LXGWWenKaiMono-Regular.woff2");

pub fn App() -> Element {
    let mut authenticated = use_signal(|| false);
    let mut connected = use_signal(|| false);
    let mut system_status = use_signal(common::SystemInfo::default);
    let mut history = use_signal(|| VecDeque::<(u64, common::SystemInfo)>::with_capacity(30 * 60));
    let mut services = use_signal(|| Vec::<common::ServiceInfo>::with_capacity(10));
    let mut refresh_interval = use_signal(|| 1000u64);
    let mut show_add_modal = use_signal(|| false);
    let mut error_msg = use_signal(|| Option::<String>::None);

    // Polling loop
    use_future(move || async move {
        loop {
            if authenticated() {
                let interval = refresh_interval();
                if interval > 0 {
                    match command::get_status(Some(interval)).await {
                        Ok(status) => {
                            system_status.set(status.clone());

                            let mut current_history = history();
                            let timestamp = status.timestamp;
                            current_history.push_back((timestamp, status));

                            // Keep only last 30 minutes of data
                            const MAX_TIME_WINDOW_MS: u64 = 30 * 60 * 1000; // 30 minutes
                            while current_history.len() > 1 {
                                if let (Some(oldest), Some(newest)) =
                                    (current_history.front(), current_history.back())
                                {
                                    if newest.0 - oldest.0 > MAX_TIME_WINDOW_MS {
                                        current_history.pop_front();
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            history.set(current_history);

                            connected.set(true);
                            if error_msg().is_some() {
                                error_msg.set(None);
                            }
                        }
                        Err(e) => {
                            connected.set(false);
                            error_msg.set(Some(format!("Connection Error: {}", e)));
                            log::error!("Failed to get status: {}", e);
                        }
                    }
                    match command::list_services().await {
                        Ok(list) => services.set(list),
                        Err(e) => log::error!("Failed to list services: {}", e),
                    }
                    gloo_timers::future::sleep(Duration::from_millis(interval)).await;
                } else {
                    gloo_timers::future::sleep(Duration::from_millis(1000)).await;
                }
            } else {
                gloo_timers::future::sleep(Duration::from_millis(100)).await;
            }
        }
    });

    let handle_login = move |(pass, addr): (String, String)| {
        spawn(async move {
            gloo_console::log!("Logging in with address: {}", &addr);
            match command::authenticate(pass, addr).await {
                Ok(_) => {
                    authenticated.set(true);
                    error_msg.set(None);
                }
                Err(e) => error_msg.set(Some(e)),
            }
        });
    };

    let handle_control = move |(id, action): (usize, String)| {
        spawn(async move {
            if let Err(e) = command::control_service(id, action).await {
                log::error!("Failed to control service: {}", e);
            } else {
                // Refresh immediately
                if let Ok(list) = command::list_services().await {
                    services.set(list);
                }
            }
        });
    };

    let handle_add_service = move |(desc, exe, args): (String, String, Vec<String>)| {
        gloo_console::log!("handle_add_service 被调用");
        spawn(async move {
            gloo_console::log!(format!(
                "准备添加服务: desc='{}', exe='{}', args={:?}",
                desc, exe, args
            ));
            match command::add_service(desc.clone(), exe.clone(), args.clone()).await {
                Ok(id) => {
                    gloo_console::log!(format!("服务添加成功, ID: {}", id));
                    show_add_modal.set(false);
                    if let Ok(list) = command::list_services().await {
                        gloo_console::log!(format!("刷新服务列表, 共 {} 个服务", list.len()));
                        services.set(list);
                    }
                }
                Err(e) => {
                    gloo_console::log!(format!("添加服务失败: {}", e));
                    log::error!("Failed to add service: {}", e);
                }
            }
        });
    };

    rsx! {
        style {
            "
            @font-face {{
                font-family: 'LXGW WenKai Mono';
                src: url('{FONT_MONO}') format('woff2');
                font-weight: normal;
                font-style: normal;
                font-display: swap;
            }}
            "
        }
        link { rel: "stylesheet", href: BASE_CSS }
        link { rel: "stylesheet", href: HEADER_CSS }
        link { rel: "stylesheet", href: LOGIN_CSS }
        link { rel: "stylesheet", href: STATUS_CSS }
        link { rel: "stylesheet", href: SERVICES_CSS }
        link { rel: "stylesheet", href: CHART_CSS }

        div { class: "app-root",

            if !authenticated() {
                Login { on_login: handle_login }
            } else {
                Header {
                    refresh_interval: refresh_interval(),
                    uptime: system_status().uptime,
                    on_refresh_change: move |val| refresh_interval.set(val),
                }

                if let Some(err) = error_msg() {
                    div { class: "error-banner",
                        svg {
                            class: "icon-sm",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z",
                            }
                        }
                        span { "{err}" }
                    }
                }

                main { class: "app-main",

                    SystemStatusDisplay { status: system_status() }

                    TrendChart { history: history() }

                    ServiceList {
                        services: services(),
                        on_control: handle_control,
                        on_add: move |_| show_add_modal.set(true),
                    }
                }

                if show_add_modal() {
                    AddServiceModal {
                        on_close: move |_| show_add_modal.set(false),
                        on_add: handle_add_service,
                    }
                }
            }
        }
    }
}
