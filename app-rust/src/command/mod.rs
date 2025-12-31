use common::{ServiceInfo, SystemInfo};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // invoke without arguments
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_without_args(cmd: &str) -> JsValue;

    // invoke with arguments (default)
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    // They need to have different names!
}

#[derive(Serialize)]
struct AuthenticateArgs {
    passphrase: String,
    address: String,
}

#[derive(Serialize)]
struct GetStatusArgs {
    interval_ms: Option<u64>,
}

#[derive(Serialize)]
struct ControlServiceArgs {
    id: usize,
    action: String,
}

#[derive(Serialize)]
struct AddServiceArgs {
    description: String,
    exe_path: String,
    args: Vec<String>,
}

pub async fn authenticate(passphrase: String, address: String) -> Result<String, String> {
    gloo_console::log!("Calling authenticate with args: {:?}", 1221);
    let args = serde_wasm_bindgen::to_value(&AuthenticateArgs {
        passphrase,
        address,
    })
    .unwrap();
    match invoke("authenticate", args).await.as_string() {
        Some(s) => Ok(s),
        None => Err("Failed to authenticate".to_string()),
    }
}

pub async fn get_status(interval_ms: Option<u64>) -> Result<SystemInfo, String> {
    let args = serde_wasm_bindgen::to_value(&GetStatusArgs { interval_ms }).unwrap();
    let result = invoke("get_status", args).await;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

pub async fn list_services() -> Result<Vec<ServiceInfo>, String> {
    let result = invoke("list_services", JsValue::NULL).await;
    // Note: The backend returns common::ServiceData, which maps to ServiceInfo
    // We need to ensure ServiceInfo matches common::ServiceData structure.
    // common::ServiceData likely has id, description, running, pid.
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

pub async fn control_service(id: usize, action: String) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&ControlServiceArgs { id, action }).unwrap();
    invoke("control_service", args).await;
    Ok(())
}

pub async fn add_service(
    description: String,
    exe_path: String,
    args: Vec<String>,
) -> Result<usize, String> {
    let args = serde_wasm_bindgen::to_value(&AddServiceArgs {
        description,
        exe_path,
        args,
    })
    .unwrap();
    let result = invoke("add_service", args).await;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

pub async fn quit_app() {
    invoke("quit_app", JsValue::NULL).await;
}
