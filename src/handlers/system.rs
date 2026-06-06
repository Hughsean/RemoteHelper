use crate::handlers::Handler;
use crate::state::AppState;
use common::{Request, Response, SystemInfo};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

pub struct SystemHandler;

impl Handler for SystemHandler {
    fn can_handle(&self, req: &Request) -> bool {
        matches!(req, Request::GetStatus { .. })
    }

    fn handle<'a>(
        &'a self,
        req: Request,
        state: AppState,
        resp_tx: mpsc::Sender<Response>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let interval_ms = match req {
                Request::GetStatus { interval_ms } => interval_ms,
                _ => None,
            };
            let response = handle_get_status(&state, interval_ms).await;
            let _ = resp_tx.send(response).await;
        })
    }
}

async fn handle_get_status(state: &AppState, interval_ms: Option<u64>) -> Response {
    // 更新最后读取时间
    {
        let mut last_read = state.last_read_time.write().await;
        *last_read = std::time::Instant::now();
    }

    // 如果提供了刷新间隔，则更新
    if let Some(ms) = interval_ms
        && ms >= 100
    {
        let current = {
            let r = state.refresh_interval.read().await;
            *r
        };
        if current != ms {
            let mut lock = state.refresh_interval.write().await;
            if *lock != ms {
                *lock = ms;
                state.update_notify.notify_one();
            }
        }
    }

    let (cpu, mem, total, uptime, cpu_model, net_tx, net_rx, net_tx_spd, net_rx_spd) = {
        let sys = state.sys.read().await;
        let networks = state.networks.read().await;
        let cpu_model = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default();

        let mut tx = 0;
        let mut rx = 0;
        let mut tx_spd = 0;
        let mut rx_spd = 0;

        for (_name, data) in networks.iter() {
            tx += data.total_transmitted();
            rx += data.total_received();
            tx_spd += data.transmitted();
            rx_spd += data.received();
        }

        (
            sys.global_cpu_usage(),
            sys.used_memory(),
            sys.total_memory(),
            sysinfo::System::uptime(),
            cpu_model,
            tx,
            rx,
            tx_spd,
            rx_spd,
        )
    };

    let (gpu_usage, gpu_memory_usage, gpu_total_memory, gpu_model) = {
        let cache = state.gpu_cache.read().await;
        (
            cache.usage,
            cache.memory_used,
            cache.memory_total,
            cache.model.clone(),
        )
    };

    let (gpu_temperature, gpu_power_watts) = {
        let cache = state.gpu_cache.read().await;
        (cache.temperature, cache.power_watts)
    };

    Response::Status(SystemInfo {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        cpu_usage: cpu,
        memory_usage: mem,
        total_memory: total,
        uptime,
        gpu_usage,
        gpu_memory_usage,
        gpu_total_memory,
        cpu_model,
        gpu_model,
        gpu_temperature,
        gpu_power_watts,
        cpu_temperature: state.get_cpu_temp(),
        cpu_package_power: state.get_cpu_power(),
        network_tx_bytes: net_tx,
        network_rx_bytes: net_rx,
        network_tx_speed: net_tx_spd,
        network_rx_speed: net_rx_spd,
    })
}
