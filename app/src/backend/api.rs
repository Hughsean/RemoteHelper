//! API 调用封装
//!
//! 提供类型安全的服务器 API 调用接口
//! 使用 client 库的全局 send_request 函数

use common::{Request, Response, ServiceAction, ServiceInfo, SystemInfo};

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
/// * `user_name` - 用户模式服务的用户名（可选）
/// * `user_password` - 用户模式服务的密码（可选）
pub async fn control_service(
    service_id: usize,
    action: String,
    user_name: Option<String>,
    user_password: Option<String>,
) -> Result<(), String> {
    tracing::info!(
        "API: control_service 被调用 - id={}, action={}, user_name={:?}",
        service_id,
        action,
        user_name
    );

    let service_action = match action.as_str() {
        "start" => ServiceAction::Start,
        "stop" => ServiceAction::Stop,
        "restart" => ServiceAction::Restart,
        _ => return Err(format!("Unknown action: {}", action)),
    };

    let request = Request::ControlService {
        id: service_id,
        action: service_action,
        user_name,
        user_password,
    };

    tracing::info!("API: 准备发送请求到服务器: {:?}", request);

    let result = client::send_request(request).await;

    tracing::info!("API: 服务器返回结果: {:?}", result);

    match result? {
        Response::Ok => {
            tracing::info!("API: 操作成功");
            Ok(())
        }
        Response::Error(msg) => {
            tracing::error!("API: 服务器返回错误: {}", msg);
            Err(msg)
        }
        _ => {
            tracing::error!("API: 收到意外的响应类型");
            Err("Unexpected response type".to_string())
        }
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
    run_as_user: bool,
    user_name: Option<String>,
    user_password: Option<String>,
) -> Result<usize, String> {
    let request = Request::AddService {
        description,
        exe_path,
        args,
        run_as_user,
        user_name,
        user_password,
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
