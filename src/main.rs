mod config;
mod email;
mod frpc;
mod logger;
mod web;

use crate::config::load_config;
use crate::frpc::FrpcManager;
use crate::logger::write_app_log;
use crate::web::WebServer;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    // 添加日志分隔符
    write_app_log("\n\n");
    write_app_log("========================================");
    write_app_log("======== APPLICATION STARTING ==========");
    write_app_log("========================================");
    write_app_log("Application starting...");

    // 等待网络就绪 - 延迟25秒确保DNS和网络服务已启动
    write_app_log("Waiting 25 seconds for network initialization...");
    // thread::sleep(Duration::from_secs(25));
    write_app_log("Network wait completed, proceeding with startup.");

    let config = match load_config() {
        Ok(c) => {
            write_app_log("Config loaded successfully.");
            c
        }
        Err(e) => {
            let err_msg = format!("Failed to load config: {}", e);
            eprintln!("{}", err_msg);
            write_app_log(&err_msg);
            return;
        }
    };

    // Initialize FrpcManager
    let frpc_manager = Arc::new(Mutex::new(FrpcManager::new(config.clone())));

    // Handle Web Control
    let mut web_server_handle = None;
    let web_server_running = Arc::new(Mutex::new(true));

    if let Some(web_conf) = config.web_control.clone() {
        if web_conf.enabled {
            write_app_log("Web control enabled. Starting Web Server...");
            let frpc_manager_clone = frpc_manager.clone();
            let web_server_running_clone = web_server_running.clone();
            
            web_server_handle = Some(thread::spawn(move || {
                let mut server = WebServer::new(web_conf, frpc_manager_clone);
                server.start();
                // If start returns, it means server stopped or error
                *web_server_running_clone.lock().unwrap() = false;
            }));
        }
    } else {
        // If no web control, start services immediately
        write_app_log("Web control disabled. Starting services immediately.");
        frpc_manager.lock().unwrap().start_all();
    }

    // Main loop to keep the application alive and handle shutdown
    let shutdown = Arc::new(Mutex::new(false));
    let shutdown_clone = shutdown.clone();

    ctrlc::set_handler(move || {
        write_app_log("Received shutdown signal (Ctrl+C or system shutdown)");
        *shutdown_clone.lock().unwrap() = true;
    })
    .expect("Error setting Ctrl-C handler");

    loop {
        if *shutdown.lock().unwrap() {
            break;
        }
        
        // Check if web server thread died unexpectedly
        if let Some(ref _handle) = web_server_handle {
             if !*web_server_running.lock().unwrap() {
                 write_app_log("Web server thread exited. Shutting down.");
                 break;
             }
        }

        // Periodic health check
        {
            let mut mgr = frpc_manager.lock().unwrap();
            mgr.check_health();
        }

        thread::sleep(Duration::from_secs(5));
    }

    // Cleanup
    write_app_log("Shutting down application...");
    {
        let mut mgr = frpc_manager.lock().unwrap();
        mgr.stop_all();
    }
    
    // Note: We can't easily stop the WebServer thread because it's blocked on accept()
    // But since we are exiting the process, it will be cleaned up by OS.
    // Ideally WebServer should have a shutdown mechanism (e.g. non-blocking accept or select)
    
    write_app_log("Application exited.");
}
