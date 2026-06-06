use crate::state::AppState;
use std::time::Duration;
use tokio::task::JoinHandle;

/// 启动后台系统监控任务。
///
/// 定期刷新 sysinfo 指标和 GPU 数据。若无客户端在最近 10 秒内
/// 读取数据则自动降频以节约资源。
pub fn spawn(state: AppState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut first_pause = false;

        loop {
            let interval_ms = *state.refresh_interval.read().await;

            // 检查是否应暂停更新（10 秒内无读取）
            let should_pause = {
                let last_read = *state.last_read_time.read().await;
                last_read.elapsed() > Duration::from_secs(10)
            };

            let timeout = tokio::select! {
                _ = if should_pause {
                    tokio::time::sleep(Duration::from_secs(3))
                }
                else {
                    tokio::time::sleep(Duration::from_millis(interval_ms))
                } => {
                    true
                    // 定时器到期，执行刷新
                }
                _ = state.update_notify.notified() => {
                    false
                    // 配置已更改，立即唤醒（并刷新）
                }
            };

            if timeout && should_pause {
                // 跳过本次周期
                if first_pause {
                    tracing::info!("监控已暂停 - 无最近读取");
                }
                first_pause = false;
                continue;
            }

            first_pause = true;

            tracing::trace!("正在刷新系统指标...");
            {
                let mut sys = state.sys.write().await;
                sys.refresh_cpu_all();
                sys.refresh_memory();
            }
            {
                let mut networks = state.networks.write().await;
                networks.refresh(true);
            }

            // 更新 GPU 缓存
            {
                let nvml_lock = state.nvml.read().await;
                let mut cache = state.gpu_cache.write().await;

                if let Some(nvml) = &*nvml_lock
                    && let Ok(device) = nvml.device_by_index(0)
                {
                    cache.usage = device.utilization_rates().map(|r| r.gpu).ok();
                    let (mem, total) = device
                        .memory_info()
                        .map(|i| (Some(i.used), Some(i.total)))
                        .unwrap_or((None, None));
                    cache.memory_used = mem;
                    cache.memory_total = total;
                    cache.model = device.name().ok();

                    // 温度（°C）
                    cache.temperature = device
                        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                        .map(|t| t as f32)
                        .ok();

                    // 功率（mW -> W）
                    cache.power_watts = device.power_usage().map(|pmw| pmw as f32 / 1000.0).ok();
                } else {
                    // 当 NVML 不可用或设备访问失败时，清空缓存中的即时值
                    cache.temperature = None;
                    cache.power_watts = None;
                }
            }
        }
    })
}
