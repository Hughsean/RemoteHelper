use common::{ServiceInfo, SystemInfo};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // invoke without arguments
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    pub async fn invoke_without_args(cmd: &str) -> Result<JsValue, JsValue>;

    // invoke with arguments (default)
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"])]
    pub async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

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
    match invoke("authenticate", args).await {
        Ok(val) => match val.as_string() {
            Some(s) => Ok(s),
            None => Err("Failed to authenticate".to_string()),
        },
        Err(e) => Err(e.as_string().unwrap_or("Unknown error".to_string())),
    }
}

pub async fn get_status(interval_ms: Option<u64>) -> Result<SystemInfo, String> {
    let args = serde_wasm_bindgen::to_value(&GetStatusArgs { interval_ms }).unwrap();
    match invoke("get_status", args).await {
        Ok(result) => serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string()),
        Err(e) => Err(e.as_string().unwrap_or("Unknown error".to_string())),
    }
}

pub async fn list_services() -> Result<Vec<ServiceInfo>, String> {
    match invoke("list_services", JsValue::NULL).await {
        Ok(result) => serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string()),
        Err(e) => Err(e.as_string().unwrap_or("Unknown error".to_string())),
    }
}

pub async fn control_service(id: usize, action: String) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&ControlServiceArgs { id, action }).unwrap();
    match invoke("control_service", args).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.as_string().unwrap_or("Unknown error".to_string())),
    }
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
    match invoke("add_service", args).await {
        Ok(result) => serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string()),
        Err(e) => Err(e.as_string().unwrap_or("Unknown error".to_string())),
    }
}

pub async fn quit_app() {
    let _ = invoke("quit_app", JsValue::NULL).await;
}
