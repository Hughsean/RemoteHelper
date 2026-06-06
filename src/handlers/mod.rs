use crate::state::AppState;
use common::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc;

mod auth;
mod path;
mod service;
mod system;

/// 请求处理器 trait。
pub trait Handler: Send + Sync + 'static {
    fn can_handle(&self, req: &Request) -> bool;

    fn handle<'a>(
        &'a self,
        req: Request,
        state: AppState,
        resp_tx: mpsc::Sender<Response>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

type BoxedHandler = Arc<dyn Handler>;

/// 写一次读多次，无需锁。
static HANDLERS: OnceLock<Vec<BoxedHandler>> = OnceLock::new();

/// 启动时调用一次。
pub fn register_default_handlers() {
    HANDLERS
        .set(vec![
            Arc::new(system::SystemHandler),
            Arc::new(service::ServiceHandler),
            Arc::new(path::PathHandler),
        ])
        .ok();
}

/// 线性扫描匹配的 handler，无锁。
/// 返回 `true` 表示找到并已派发。
pub fn dispatch(
    req: Request,
    state: AppState,
    resp_tx: mpsc::Sender<Response>,
) -> bool {
    let Some(handlers) = HANDLERS.get() else {
        tracing::error!("handlers not registered — missing register_default_handlers() call at startup");
        return false;
    };
    for handler in handlers {
        if handler.can_handle(&req) {
            let handler = Arc::clone(handler);
            tokio::spawn(async move {
                handler.handle(req, state, resp_tx).await;
            });
            return true;
        }
    }
    false
}

// Re-export for connection loop
pub use auth::verify_login;
