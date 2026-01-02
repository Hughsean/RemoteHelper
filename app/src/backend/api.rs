//! API 调用封装
//!
//! 提供类型安全的服务器 API 调用接口
//! 使用 client 库的全局 send_request 函数

use common::{Request, Response, ServiceInfo, SystemInfo, ServiceAction};

/// 认证到服务器
///
/// # Arguments
/// * `password` - 私钥解密密码
/// * `addr` - 服务器地址 (如 "127.0.0.1:9999")
pub async fn authenticate(password: String, addr: String) -> Result<(), String> {
    super::connect(addr, password).await
}

/// 获取系统状态
///
/// # Arguments
/// * `interval_ms` - 监控刷新间隔（毫秒）
pub async fn get_status(interval_ms: Option<u64>) -> Result<SystemInfo, String> {
    let request = Request::GetStatus { interval_ms };

    match client::send_request(request).await? {
        Response::Status(status) => Ok(status),
        Response::Error(msg) => Err(msg),
        _ => Err("Unexpected response type".to_string()),
    }
}

/// 获取服务列表
pub async fn list_services() -> Result<Vec<ServiceInfo>, String> {
    let request = Request::ListServices;

    match client::send_request(request).await? {
        Response::Services(services) => Ok(services),
        Response::Error(msg) => Err(msg),
        _ => Err("Unexpected response type".to_string()),
    }
}

/// 控制服务（启动/停止/重启）
///
/// # Arguments
/// * `service_id` - 服务ID
/// * `action` - 操作类型: "start", "stop", "restart"
pub async fn control_service(service_id: usize, action: String) -> Result<(), String> {
    let service_action = match action.as_str() {
        "start" => ServiceAction::Start,
        "stop" => ServiceAction::Stop,
        "restart" => ServiceAction::Restart,
        _ => return Err(format!("Unknown action: {}", action)),
    };

    let request = Request::ControlService {
        id: service_id,
        action: service_action
    };

    match client::send_request(request).await? {
        Response::Ok => Ok(()),
        Response::Error(msg) => Err(msg),
        _ => Err("Unexpected response type".to_string()),
    }
}

/// 添加动态服务
///
/// # Arguments
/// * `description` - 服务描述
/// * `exe_path` - 可执行文件路径
/// * `args` - 启动参数
pub async fn add_service(
    description: String,
    exe_path: String,
    args: Vec<String>,
) -> Result<usize, String> {
    let request = Request::AddService {
        description,
        exe_path,
        args,
    };

    match client::send_request(request).await? {
        Response::ServiceAdded(id) => Ok(id),
        Response::Error(msg) => Err(msg),
        _ => Err("Unexpected response type".to_string()),
    }
}

/// 查询路径建议（用于文件选择器）
pub async fn query_path(path: String) -> Result<Vec<common::PathItem>, String> {
    client::query_path(path).await
}
