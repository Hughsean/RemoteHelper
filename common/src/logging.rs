//! 日志系统初始化。
//!
//! 提供统一的 `tracing` 初始化函数，支持同时输出到 stdout 和文件。

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// 初始化 tracing 日志系统。
///
/// # 参数
/// - `dir`: 可选的文件日志目录
/// - `file`: 可选的文件日志文件名
/// - `lvl`: 最低日志级别
///
/// # 返回值
/// 返回 `Some(WorkerGuard)` 如果启用了文件日志，否则返回 `None`。
/// 返回的 guard 必须保留以便后续同步缓冲区到文件。
#[must_use]
pub fn tracing_init(
    dir: Option<&str>,
    file: Option<&str>,
    lvl: tracing::Level,
) -> Option<WorkerGuard> {
    // 创建标准输出日志层
    let timer = tracing_subscriber::fmt::time::OffsetTime::new(
        time::UtcOffset::current_local_offset().expect("failed to get local time offset"),
        time::macros::format_description!("[hour]:[minute]:[second]:[subsecond digits:3]"),
    );

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(std::io::stdout)
        .with_thread_ids(true)
        .with_timer(timer.clone())
        .with_target(false);

    // 创建环境过滤器
    let env_filter = tracing_subscriber::filter::LevelFilter::from_level(lvl);

    if let (Some(dir), Some(file)) = (dir, file) {
        let file_appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(file)
            .filename_suffix("log")
            .build(dir)
            .expect("failed to init log appender");

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let file_layer = tracing_subscriber::fmt::layer()
            .with_file(true)
            .with_line_number(true)
            .with_ansi(false)
            .with_thread_ids(true)
            .with_target(false)
            .with_writer(non_blocking)
            .with_timer(timer);

        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .with(env_filter)
            .init();

        Some(guard)
    } else {
        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(env_filter)
            .init();

        None
    }
}
