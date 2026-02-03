mod config;
mod process;
mod server;
mod state;
mod utils;

use crate::config::AppConfig;
use crate::state::AppState;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // 初始化 tracing
    #[cfg(debug_assertions)]
    let _guard =
        common::func::tracing_init(Some("logs"), Some("server.log"), tracing::Level::DEBUG);
    #[cfg(not(debug_assertions))]
    let _guard = common::func::tracing_init(Some("logs"), Some("server.log"), tracing::Level::INFO);

    tracing::info!("正在启动 RemoteHelper 服务器实例");

    // 加载配置
    let config = AppConfig::load()?;

    let no_url = config.web_panel.health_check_url.is_none();
    let no_delay = config.web_panel.startup_delay_secs == 0;

    if !no_url || !no_delay {
        tracing::info!(
            "等待网络初始化... {} 秒",
            config.web_panel.startup_delay_secs
        );

        let delay = config.web_panel.startup_delay_secs;

        if !no_delay && no_url {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        } else if !no_url {
            let timeout = delay.max(3);
            let mut sleep_time = 3;
            loop {
                let connected = utils::test_http_503(
                    config.web_panel.health_check_url.as_ref().unwrap(),
                    timeout,
                )
                .await
                .unwrap_or(false);

                if connected {
                    tracing::info!("检测到网络初始化完成");
                    break;
                };
                tracing::warn!("网络尚未初始化完成，{} 秒后重试...", sleep_time);
                tokio::time::sleep(Duration::from_secs(sleep_time)).await;
                sleep_time = (sleep_time * 2).min(timeout * 6);
            }
        }
    }
    tracing::info!("配置文件加载成功");
    tracing::info!(
        "Web 面板配置 - 启用: {}, 端口: {}, 最大连接数: {}",
        config.web_panel.enabled,
        config.web_panel.local_port,
        config.web_panel.max_connections
    );
    tracing::info!("已配置服务数量: {}", config.service.len());

    // 初始化状态
    let state = AppState::new(config.clone());
    tracing::info!("应用程序状态初始化完成");

    // 启动后台监控任务
    let monitor_state = state.clone();
    tokio::spawn(async move {
        let mut first_pause = false;

        match hwlib::driver::init_driver() {
            Ok(_) => tracing::info!("hwlib 驱动初始化成功"),
            Err(e) => tracing::warn!("hwlib 驱动初始化失败: {}", e),
        }

        let mut sensor_hub = Some(hwlib::Sensors::new());
        let pm = match hwlib::driver::get_driver().and_then(|d| d.pawn_manager()) {
            Ok(pm) => Some(pm),
            Err(e) => {
                tracing::warn!("获取 hwlib pawn manager 失败: {}", e);
                None
            }
        };

        if let (Some(hub), Some(pm_)) = (sensor_hub.as_mut(), pm) {
            hub.set_pawn_manager(pm_.clone());
            tracing::info!("SensorHub pawn manager 设置完成");
            match hub.detect() {
                Ok(_) => tracing::info!("SensorHub 硬件检测成功"),
                Err(e) => tracing::warn!("SensorHub 硬件检测失败: {}", e),
            }
        }

        loop {
            let interval_ms = *monitor_state.refresh_interval.read().await;

            // 检查是否应暂停更新（10 秒内无读取）
            let should_pause = {
                let last_read = *monitor_state.last_read_time.read().await;
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
                _ = monitor_state.update_notify.notified() => {
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
                let mut sys = monitor_state.sys.write().await;
                sys.refresh_cpu_all();
                sys.refresh_memory();
            }
            {
                let mut networks = monitor_state.networks.write().await;
                networks.refresh(true);
            }

            // 更新 GPU 缓存
            {
                let nvml_lock = monitor_state.nvml.read().await;
                let mut cache = monitor_state.gpu_cache.write().await;

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

            if let Some(hub) = &mut sensor_hub
                && let Ok((cpu_readings, _mb)) = hub.read_all()
            {
                if cpu_readings.is_empty() {
                    tracing::trace!("SensorHub returned no CPU sensors (worker)");
                }

                let _c = cpu_readings
                    .iter()
                    .scan((0, 0), |s, x| {
                        if s.0 > 0 && s.1 > 0 {
                            return None;
                        }
                        // Update temperature cache
                        if s.0 == 0
                            && x.sensor_type == hwlib::core::SensorType::Temperature
                            && let Some(v) = &x.value
                        {
                            monitor_state.set_cpu_temp(Some(v.value));
                            s.0 += 1;
                        }
                        // Update power cache
                        else if s.1 == 0
                            && x.sensor_type == hwlib::core::SensorType::Power
                            && let Some(v) = &x.value
                        {
                            tracing::debug!("更新 CPU 功率传感器值: {} W", v.value);
                            monitor_state.set_cpu_power(Some(v.value));
                            s.1 += 1;
                        } else {
                            tracing::trace!("Power sensor present but has no value: {}", x.name);
                        }
                        Some((s.0, s.1))
                    })
                    .count();
                // tracing::debug!("readings processed: {}", c);
                tracing::trace!(
                    "当前 CPU 温度缓存: {:?} °C, 功率缓存: {:?} W",
                    monitor_state.get_cpu_temp(),
                    monitor_state.get_cpu_power()
                );
            }
        }
    });

    // 自动启动服务
    tracing::info!("正在启动自动启动服务...");
    let auto_start_count = state
        .config
        .service
        .iter()
        .filter(|s| s.auto_start != crate::config::AutoStart::None)
        .count();
    tracing::info!("发现 {} 个标记为自动启动的服务", auto_start_count);

    for (id, svc) in state.config.service.iter().enumerate() {
        match svc.auto_start.clone() {
            crate::config::AutoStart::None => {}
            crate::config::AutoStart::OneShot => {
                tracing::info!("OneShot 自动启动服务 ID {}: {}", id, svc.description);
                if let Err(e) = process::start_service(&state, id).await {
                    tracing::error!("自动启动服务失败 {} ({}): {}", id, svc.description, e);
                } else {
                    tracing::info!("成功 OneShot 启动服务 ID {}", id);
                }
            }
            crate::config::AutoStart::Continuous => {
                tracing::info!(
                    "Continuous 自动启动服务 ID {}: {}（持久运行，保留 PID 以便后续销毁）",
                    id,
                    svc.description
                );
                if let Err(e) = process::start_service(&state, id).await {
                    tracing::error!("自动启动服务失败 {} ({}): {}", id, svc.description, e);
                } else {
                    tracing::info!("成功 Continuous 启动服务 ID {}", id);
                }
            }
        }
    }

    // 自动启动 Web 隧道
    tracing::info!("正在启动 Web 隧道...");
    if let Err(e) = process::start_web_tunnel(&state).await {
        tracing::error!("启动 Web 隧道失败: {}", e);
    } else {
        tracing::info!("Web 隧道启动成功");
    }

    // 为清理保留一个克隆
    let cleanup_state = state.clone();

    // 运行服务器
    let port = config.web_panel.local_port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("正在监听 {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // 接收循环
    let server_state = state.clone();
    let max_connections = config.web_panel.max_connections;
    tokio::select! {
        _ = async {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        // 检查连接上限
                        let current = server_state.active_connections.load(std::sync::atomic::Ordering::Relaxed);
                        if current >= max_connections {
                            tracing::warn!("连接数已达上限 ({}), 拒绝来自 {} 的连接", max_connections, addr);
                            drop(socket);
                            continue;
                        }

                        tracing::info!("来自 {} 的新连接 ({}/{})", addr, current + 1, max_connections);

                        let state = server_state.clone();
                        tokio::spawn(async move {
                            server::handle_connection(socket, state).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("接受连接失败: {}", e);
                    }
                }
            }
        } => {},
        _ = shutdown_signal() => {},
    }

    // 清理逻辑
    tracing::info!("正在关闭，终止子进程...");
    let active = cleanup_state
        .active_connections
        .load(std::sync::atomic::Ordering::Relaxed);
    if active > 0 {
        tracing::warn!("关闭时仍有 {} 个活动连接", active);
    }

    // 终止服务进程（带超时）
    {
        let mut processes = cleanup_state.service_processes.write().await;
        tracing::info!("发现 {} 个需要终止的服务进程", processes.len());
        for (id, sp) in processes.iter_mut() {
            tracing::info!("正在终止服务进程 {}", id);
            if let Err(e) = sp.start_kill() {
                tracing::error!("终止服务进程 {} 失败: {}", id, e);
                continue;
            }
            // 等待进程退出（5 秒超时）
            match tokio::time::timeout(Duration::from_secs(5), sp.wait()).await {
                Ok(Ok(status)) => {
                    tracing::info!("服务 {} 退出，状态: {:?}", id, status)
                }
                Ok(Err(e)) => tracing::error!("等待服务 {} 时出错: {}", id, e),
                Err(_) => {
                    tracing::warn!("服务 {} 未在超时时间内退出，强制终止", id);
                    let _ = sp.kill().await;
                }
            }
        }
    }

    // 终止 Web 隧道（带超时）
    {
        let mut tunnel = cleanup_state.web_tunnel_process.lock().await;
        if let Some(child) = tunnel.as_mut() {
            tracing::info!("正在终止 Web 隧道进程");
            if let Err(e) = child.start_kill() {
                tracing::error!("终止 Web 隧道失败: {}", e);
            } else {
                match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                    Ok(Ok(status)) => tracing::info!("Web 隧道退出，状态: {:?}", status),
                    Ok(Err(e)) => tracing::error!("等待 Web 隧道时出错: {}", e),
                    Err(_) => {
                        tracing::warn!("Web 隧道未在超时时间内退出，强制终止");
                        let _ = child.kill().await;
                    }
                }
            }
        }
    }
    tracing::info!("关闭完成\n\n\n\n");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("收到信号，开始优雅关闭");
}
