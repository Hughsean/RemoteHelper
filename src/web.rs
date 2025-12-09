use crate::config::WebControlConfig;
use crate::frpc::FrpcManager;
use crate::logger::write_app_log;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// 安全配置常量
const MAX_REQUEST_SIZE: usize = 4096; // 限制请求头+体最大 4KB
const READ_TIMEOUT: u64 = 5; // 读取超时 5秒
const MAX_FAILED_ATTEMPTS: u32 = 5; // 最大失败尝试次数
const LOCKOUT_DURATION: u64 = 300; // 封锁时间 300秒 (5分钟)

struct IpStatus {
    failed_attempts: u32,
    lockout_until: Option<Instant>,
}

pub struct WebServer {
    config: WebControlConfig,
    frpc_manager: Arc<Mutex<FrpcManager>>,
    tunnel_process: Option<Child>,
    ip_tracker: Arc<Mutex<HashMap<String, IpStatus>>>,
}

impl WebServer {
    pub fn new(config: WebControlConfig, frpc_manager: Arc<Mutex<FrpcManager>>) -> Self {
        Self {
            config,
            frpc_manager,
            tunnel_process: None,
            ip_tracker: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&mut self) {
        let local_port = self.config.local_port;
        let frpc_arg = self.config.frpc_arg.clone();

        // 1. Start frpc tunnel for web control
        write_app_log("Starting frpc tunnel for web control...");
        match Command::new("frpc.exe").arg("-f").arg(&frpc_arg).spawn() {
            Ok(c) => self.tunnel_process = Some(c),
            Err(e) => {
                write_app_log(&format!("Failed to start web control tunnel: {}", e));
                return;
            }
        }

        // 2. Start HTTP Server
        let listener = match TcpListener::bind(format!("0.0.0.0:{}", local_port)) {
            Ok(l) => l,
            Err(e) => {
                write_app_log(&format!("Failed to bind web control port: {}", e));
                return;
            }
        };

        write_app_log(&format!("Web Control listening on port {}", local_port));

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    // 设置读取超时，防止 Slowloris 攻击
                    if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(READ_TIMEOUT)))
                    {
                        write_app_log(&format!("Failed to set read timeout: {}", e));
                        continue;
                    }

                    // 获取客户端 IP
                    let peer_addr = stream.peer_addr().ok();
                    let ip = peer_addr
                        .map(|a| a.ip().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    // 检查 IP 是否被封锁
                    if self.is_ip_locked(&ip) {
                        write_app_log(&format!("Rejected connection from locked IP: {}", ip));
                        continue; // 直接断开连接，不返回任何数据
                    }

                    let mut buffer = [0; MAX_REQUEST_SIZE];
                    // 限制读取大小
                    match stream.read(&mut buffer) {
                        Ok(n) if n > 0 => {
                            let request = String::from_utf8_lossy(&buffer[..n]);
                            self.handle_request(&request, &mut stream, &ip);
                        }
                        Ok(_) => {
                            // 0 bytes read, connection closed
                        }
                        Err(e) => {
                            // Timeout or other error
                            write_app_log(&format!("Connection error from {}: {}", ip, e));
                        }
                    }
                }
                Err(e) => {
                    write_app_log(&format!("Connection accept error: {}", e));
                }
            }
        }
    }

    fn is_ip_locked(&self, ip: &str) -> bool {
        let mut tracker = self.ip_tracker.lock().unwrap();
        if let Some(status) = tracker.get_mut(ip) {
            if let Some(lockout_time) = status.lockout_until {
                if Instant::now() < lockout_time {
                    return true;
                } else {
                    // 封锁过期，重置
                    status.failed_attempts = 0;
                    status.lockout_until = None;
                }
            }
        }
        false
    }

    fn record_failed_attempt(&self, ip: &str) {
        let mut tracker = self.ip_tracker.lock().unwrap();
        let status = tracker.entry(ip.to_string()).or_insert(IpStatus {
            failed_attempts: 0,
            lockout_until: None,
        });

        status.failed_attempts += 1;
        if status.failed_attempts >= MAX_FAILED_ATTEMPTS {
            status.lockout_until = Some(Instant::now() + Duration::from_secs(LOCKOUT_DURATION));
            write_app_log(&format!(
                "IP {} locked out due to too many failed attempts.",
                ip
            ));
        }
    }

    fn reset_failed_attempts(&self, ip: &str) {
        let mut tracker = self.ip_tracker.lock().unwrap();
        if let Some(status) = tracker.get_mut(ip) {
            status.failed_attempts = 0;
            status.lockout_until = None;
        }
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) {
        if let Some(mut child) = self.tunnel_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn handle_request(&self, request: &str, stream: &mut TcpStream, client_ip: &str) {
        // Simple parsing
        let first_line = request.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 {
            return;
        }
        let method = parts[0];
        let path = parts[1];

        // Parse body for POST
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");

        if method == "GET" && path == "/" {
            self.serve_dashboard(stream, "");
        } else if method == "POST" && path == "/action" {
            self.handle_action(body, stream, client_ip);
        } else {
            let response = "HTTP/1.1 404 Not Found\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        }
    }

    fn handle_action(&self, body: &str, stream: &mut TcpStream, client_ip: &str) {
        // Parse body: action=start&secret=xxx
        let params: HashMap<String, String> = body
            .split('&')
            .filter_map(|s| {
                let mut split = s.split('=');
                if let (Some(k), Some(v)) = (split.next(), split.next()) {
                    Some((k.to_string(), v.to_string()))
                } else {
                    None
                }
            })
            .collect();

        let secret = params.get("secret").map(|s| s.as_str()).unwrap_or("");
        let action = params.get("action").map(|s| s.as_str()).unwrap_or("");

        if secret != self.config.auth_secret {
            self.record_failed_attempt(client_ip);
            write_app_log(&format!("Invalid secret attempt from IP: {}", client_ip));
            // 故意延迟响应，防止计时攻击
            std::thread::sleep(Duration::from_millis(1000));
            self.serve_dashboard(stream, "Invalid Secret!");
            return;
        }

        // 验证成功，重置计数器
        self.reset_failed_attempts(client_ip);

        let mut mgr = self.frpc_manager.lock().unwrap();
        match action {
            "start" => {
                mgr.start_services();
                self.serve_dashboard(stream, "Services Started");
            }
            "stop" => {
                mgr.stop_services();
                self.serve_dashboard(stream, "Services Stopped");
            }
            _ => {
                self.serve_dashboard(stream, "Unknown Action");
            }
        }
    }

    fn serve_dashboard(&self, stream: &mut TcpStream, message: &str) {
        let mgr = self.frpc_manager.lock().unwrap();
        let is_running = mgr.is_running();
        let status_text = if is_running {
            "<span style='color:green'>RUNNING</span>"
        } else {
            "<span style='color:red'>STOPPED</span>"
        };

        let html = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>FrpcStartup Control</title>
    <style>
        body {{ font-family: sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; background: #f0f2f5; margin: 0; }}
        .card {{ background: white; padding: 2rem; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); text-align: center; width: 300px; }}
        .status {{ font-size: 1.2rem; margin-bottom: 20px; }}
        input {{ width: 100%; padding: 10px; margin-bottom: 10px; box-sizing: border-box; border: 1px solid #ccc; border-radius: 4px; }}
        button {{ width: 100%; padding: 10px; font-size: 1rem; border-radius: 4px; cursor: pointer; border: none; margin-bottom: 5px; }}
        .btn-start {{ background: #28a745; color: white; }}
        .btn-stop {{ background: #dc3545; color: white; }}
        .message {{ color: blue; margin-bottom: 10px; }}
    </style>
</head>
<body>
    <div class="card">
        <h1>Frpc Control</h1>
        <div class="status">Status: {}</div>
        <div class="message">{}</div>
        <form method="POST" action="/action">
            <input type="password" name="secret" placeholder="Enter Secret" required>
            <button type="submit" name="action" value="start" class="btn-start">Start Services</button>
            <button type="submit" name="action" value="stop" class="btn-stop">Stop Services</button>
        </form>
    </div>
</body>
</html>
"#,
            status_text, message
        );

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes());
    }
}
