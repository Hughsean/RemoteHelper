#![allow(non_snake_case)]

use crate::components::{
    add_service_modal::AddServiceModal, header::Header, login::Login, service_list::ServiceList,
    system_status::SystemStatusDisplay,
};
use crate::models::{ServiceInfo, SystemInfo};
use crate::services;
use dioxus::prelude::*;
use std::time::Duration;

static CSS: Asset = asset!("/assets/styles.css");

pub fn App() -> Element {
    let mut authenticated = use_signal(|| false);
    let mut system_status = use_signal(|| SystemInfo::default());
    let mut services = use_signal(|| Vec::<ServiceInfo>::new());
    let mut refresh_interval = use_signal(|| 1000u64);
    let mut show_add_modal = use_signal(|| false);
    let mut error_msg = use_signal(|| Option::<String>::None);

    // Polling loop
    use_future(move || async move {
        loop {
            if authenticated() {
                let interval = refresh_interval();
                if interval > 0 {
                    match services::get_status(Some(interval)).await {
                        Ok(status) => system_status.set(status),
                        Err(e) => log::error!("Failed to get status: {}", e),
                    }
                    match services::list_services().await {
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
            match services::authenticate(pass, addr).await {
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
            if let Err(e) = services::control_service(id, action).await {
                log::error!("Failed to control service: {}", e);
            } else {
                // Refresh immediately
                if let Ok(list) = services::list_services().await {
                    services.set(list);
                }
            }
        });
    };

    let handle_add_service = move |(desc, exe, args): (String, String, Vec<String>)| {
        spawn(async move {
            match services::add_service(desc, exe, args).await {
                Ok(_) => {
                    show_add_modal.set(false);
                    if let Ok(list) = services::list_services().await {
                        services.set(list);
                    }
                }
                Err(e) => log::error!("Failed to add service: {}", e),
            }
        });
    };

    rsx! {
        link { rel: "stylesheet", href: CSS }
        // Tailwind CDN for development if local build not set up
        script { src: "https://cdn.tailwindcss.com" }

        div { class: "bg-slate-950 text-slate-200 font-sans min-h-screen flex flex-col selection:bg-indigo-500/30 selection:text-indigo-200",

            if !authenticated() {
                Login { on_login: handle_login }
            } else {
                Header { on_refresh_change: move |val| refresh_interval.set(val) }

                main { class: "flex-1 p-6 max-w-7xl mx-auto w-full",

                    SystemStatusDisplay { status: system_status() }

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
