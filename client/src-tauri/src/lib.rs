mod client;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_log::Builder::default().build())
    .invoke_handler(tauri::generate_handler![
        client::authenticate,
        client::get_status,
        client::list_services,
        client::control_service
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
