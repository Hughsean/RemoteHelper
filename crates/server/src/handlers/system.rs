use crate::state::AppState;
use common::Response;

pub(crate) async fn get_status(state: &AppState) -> Response {
    match state.monitor.snapshot().await {
        Some(snapshot) => Response::Status((*snapshot).clone()),
        // 使用旧协议已有的错误类型，保持旧客户端兼容。
        None => Response::Error("SampleNotReady: 采样尚未就绪，请重试".into()),
    }
}
