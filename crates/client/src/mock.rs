//! Mock 数据生成模块 —— 离线开发与调试用途。
//!
//! 当客户端处于 Mock 模式时，无需连接服务器即可获得
//! 基于正弦波合成的伪实时系统监控数据。

use common::SystemInfo;
use std::time::SystemTime;

/// 生成一组模拟的 `SystemInfo` 数据。
///
/// 使用多个不同频率/相位的正弦波叠加，模拟 CPU、内存、
/// GPU 使用率和网络速率的变化趋势，便于前端离线开发和演示。
pub fn generate_mock_system_info() -> SystemInfo {
    let start = SystemTime::now();
    let elapsed = start
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let t = elapsed;
    let cpu = 25.0 + 20.0 * (t / 3.0).sin() + 15.0 * (t / 1.7).sin();
    let mem_ratio = 0.55 + 0.15 * (t / 8.0).sin();
    let gpu = 35.0 + 25.0 * (t / 5.0).sin() + 20.0 * (t / 2.3).sin();
    let gpu_mem_ratio = 0.5 + 0.25 * (t / 7.0).sin();

    let total_mem: u64 = 16 * 1024 * 1024 * 1024;
    let gpu_total_mem: u64 = 8 * 1024 * 1024 * 1024;

    SystemInfo {
        timestamp: std::time::UNIX_EPOCH
            .elapsed()
            .unwrap_or_default()
            .as_millis() as u64,
        cpu_usage: cpu.clamp(0.0, 100.0) as f32,
        memory_usage: (total_mem as f64 * mem_ratio.clamp(0.1, 0.95)) as u64,
        total_memory: total_mem,
        uptime: (t / 3600.0) as u64,
        gpu_usage: Some(gpu.clamp(0.0, 100.0) as u32),
        gpu_memory_usage: Some((gpu_total_mem as f64 * gpu_mem_ratio.clamp(0.1, 0.95)) as u64),
        gpu_total_memory: Some(gpu_total_mem),
        cpu_model: "Mock CPU @ 3.50GHz (Debug Mode)".to_string(),
        gpu_model: Some("Mock GPU (Debug Mode)".to_string()),
        gpu_temperature: Some((55.0 + 20.0 * (t / 8.0).sin()) as f32),
        gpu_power_watts: Some((80.0 + 40.0 * (t / 6.0).sin()) as f32),
        cpu_temperature: Some((45.0 + 15.0 * (t / 10.0).sin()) as f32),
        cpu_package_power: Some((15.0 + 10.0 * (t / 4.0).sin()) as f32),
        network_tx_bytes: (t * 10_000_000.0) as u64,
        network_rx_bytes: (t * 50_000_000.0) as u64,
        network_tx_speed: (2_000_000.0 + 18_000_000.0 * (t / 3.0).sin().abs()) as u64,
        network_rx_speed: (5_000_000.0 + 45_000_000.0 * (t / 4.0).sin().abs()) as u64,
    }
}
