use crate::state::AppState;
use common::{Request, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::mpsc;

mod auth;
mod path;
mod service;
mod system;

/// 请求处理器 trait。
///
/// 每个 handler 负责处理一种或多种请求类型。
/// Handler 实例是全局共享的（通过 Arc），因此必须是无状态的；
/// 每条连接的状态（如认证标志）由连接层独立维护。
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

static REGISTRY: LazyLock<Mutex<Vec<BoxedHandler>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn register(h: BoxedHandler) {
    REGISTRY.lock().unwrap().push(h);
}

/// 在启动时调用一次，注册所有默认处理器。
pub fn register_default_handlers() {
    register(Arc::new(system::SystemHandler));
    register(Arc::new(service::ServiceHandler));
    register(Arc::new(path::PathHandler));
}

/// 将请求分派给第一个匹配的 handler。
/// 返回 `true` 表示找到了对应的 handler。
pub fn dispatch(req: Request, state: AppState, resp_tx: mpsc::Sender<Response>) -> bool {
    let registry = REGISTRY.lock().unwrap();
    for handler in registry.iter() {
        if handler.can_handle(&req) {
            let handler = Arc::clone(handler);
            let tx = resp_tx.clone();
            tokio::spawn(async move {
                handler.handle(req, state, tx).await;
            });
            return true;
        }
    }
    false
}

// Re-export for connection loop
pub use auth::verify_login;
