#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod assets;
mod bootstrap;
mod notifications;
#[cfg(debug_assertions)]
mod preview;
mod shell;
mod theme;
mod ui;
mod views;

fn main() {
    let _log = common::logging::tracing_init(None, None, tracing::Level::INFO);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法启动网络运行时");
    bootstrap::run(runtime.handle().clone());
}
