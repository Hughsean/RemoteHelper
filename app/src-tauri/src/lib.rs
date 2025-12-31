mod command;

use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                // When another instance is opened, bring the existing window
                // to front. On macOS we must set Regular activation policy to
                // allow focusing and native full-screen.
                #[cfg(target_os = "macos")]
                {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
                }
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            // macOS: don't force Accessory policy at startup here. We'll
            // switch activation policy at runtime when showing/hiding the
            // main window so the app supports native full-screen when shown
            // while still behaving like a menu-bar accessory when hidden.

            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "显示/隐藏", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No default window icon found"))?;
            let _tray = TrayIconBuilder::with_id("tray")
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                // Hide: switch back to Accessory so Dock stays hidden
                                #[cfg(target_os = "macos")]
                                {
                                    let _ = app
                                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                                }
                                let _ = window.hide();
                            } else {
                                // Showing window: make app Regular so native full-screen works
                                #[cfg(target_os = "macos")]
                                {
                                    let _ =
                                        app.set_activation_policy(tauri::ActivationPolicy::Regular);
                                }
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                // Hide: switch back to Accessory so Dock stays hidden
                                #[cfg(target_os = "macos")]
                                {
                                    let _ = app
                                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                                }
                                let _ = window.hide();
                            } else {
                                // Showing window: make app Regular so native full-screen works
                                #[cfg(target_os = "macos")]
                                {
                                    let _ =
                                        app.set_activation_policy(tauri::ActivationPolicy::Regular);
                                }
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            if let Some(main_window) = app.get_webview_window("main") {
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if let Err(e) = main_window.show() {
                        log::error!("Failed to show main window: {}", e);
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // When window is closed (hidden), on macOS switch to Accessory
                // so the app behaves like a menu-bar accessory.
                #[cfg(target_os = "macos")]
                {
                    let app = window.app_handle();
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
                if let Err(e) = window.hide() {
                    log::error!("Failed to hide window: {}", e);
                }
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            command::authenticate,
            command::get_status,
            command::list_services,
            command::control_service,
            command::add_service,
            command::quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
