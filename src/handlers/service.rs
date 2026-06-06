use crate::handlers::Handler;
use crate::state::AppState;
use common::{Request, Response, ServiceAction, ServiceInfo};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

pub struct ServiceHandler;

impl Handler for ServiceHandler {
    fn can_handle(&self, req: &Request) -> bool {
        matches!(
            req,
            Request::ListServices | Request::ControlService { .. } | Request::AddService { .. }
        )
    }

    fn handle<'a>(
        &'a self,
        req: Request,
        state: AppState,
        resp_tx: mpsc::Sender<Response>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let response = match req {
                Request::ListServices => list_services(&state).await,
                Request::ControlService { id, action } => control_service(&state, id, action).await,
                Request::AddService {
                    description,
                    exe_path,
                    args,
                } => add_service(&state, description, exe_path, args).await,
                _ => unreachable!(),
            };
            let _ = resp_tx.send(response).await;
        })
    }
}

async fn list_services(state: &AppState) -> Response {
    tracing::debug!("正在列出所有服务");
    let mut services = Vec::new();
    let processes = state.service_processes.read().await;

    // 静态服务
    for (id, svc_config) in state.config.service.iter().enumerate() {
        let running = processes.contains_key(&id);
        let pid = processes.get(&id).and_then(|sp| sp.pid());
        services.push(ServiceInfo {
            id,
            description: svc_config.description.clone(),
            running,
            pid,
        });
    }

    // 动态服务
    let dynamic = state.dynamic_services.read().await;
    let offset = state.config.service.len();
    for (i, svc_config) in dynamic.iter().enumerate() {
        let id = offset + i;
        let running = processes.contains_key(&id);
        let pid = processes.get(&id).and_then(|sp| sp.pid());
        services.push(ServiceInfo {
            id,
            description: svc_config.description.clone(),
            running,
            pid,
        });
    }

    Response::Services(services)
}

async fn control_service(state: &AppState, id: usize, action: ServiceAction) -> Response {
    tracing::info!("服务控制请求 - ID: {}, 操作: {:?}", id, action);
    let static_count = state.config.service.len();
    let dynamic_count = state.dynamic_services.read().await.len();

    if id >= static_count + dynamic_count {
        return Response::Error("服务 ID 超出范围".to_string());
    }

    let allowed = if id < static_count {
        state.config.service[id].allow_web_control
    } else {
        true
    };

    if !allowed {
        return Response::Error("此服务的 Web 控制已禁用".to_string());
    }

    match action {
        ServiceAction::Start => match crate::process::start_service(state, id).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e.to_string()),
        },
        ServiceAction::Stop => match crate::process::stop_service(state, id).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e.to_string()),
        },
        ServiceAction::Restart => match crate::process::restart_service(state, id).await {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error(e.to_string()),
        },
    }
}

async fn add_service(
    state: &AppState,
    description: String,
    exe_path: String,
    args: Vec<String>,
) -> Response {
    tracing::info!("正在添加新服务: {} (可执行文件: {})", description, exe_path);
    let mut dynamic = state.dynamic_services.write().await;
    dynamic.push(crate::config::ServiceConfig {
        description,
        exe_path,
        args,
        auto_start: crate::config::AutoStart::None,
        allow_web_control: true,
    });

    let id = state.config.service.len() + dynamic.len() - 1;
    Response::ServiceAdded(id)
}
