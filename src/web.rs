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
            self.serve_dashboard(stream, "密钥错误！");
            return;
        }

        // 验证成功，重置计数器
        self.reset_failed_attempts(client_ip);

        let mut mgr = self.frpc_manager.lock().unwrap();
        
        if action == "start_all" {
            mgr.start_all();
            self.serve_dashboard(stream, "所有服务已启动");
        } else if action == "stop_all" {
            mgr.stop_all();
            self.serve_dashboard(stream, "所有服务已停止");
        } else if let Some(idx_str) = action.strip_prefix("start_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                mgr.start_service(idx);
                self.serve_dashboard(stream, &format!("服务 #{} 已启动", idx));
            } else {
                self.serve_dashboard(stream, "无效的服务索引");
            }
        } else if let Some(idx_str) = action.strip_prefix("stop_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                mgr.stop_service(idx);
                self.serve_dashboard(stream, &format!("服务 #{} 已停止", idx));
            } else {
                self.serve_dashboard(stream, "无效的服务索引");
            }
        } else {
            self.serve_dashboard(stream, "未知操作");
        }
    }

    fn serve_dashboard(&self, stream: &mut TcpStream, message: &str) {
        let mut mgr = self.frpc_manager.lock().unwrap();
        let statuses = mgr.get_all_statuses();
        
        let mut rows_html = String::new();
        for (i, (desc, is_running)) in statuses.iter().enumerate() {
            let status_class = if *is_running { "status-running" } else { "status-stopped" };
            let status_text = if *is_running { "运行中" } else { "已停止" };
            let action_btn = if *is_running {
                format!(r#"<button type="submit" name="action" value="stop_{}" class="btn-sm btn-stop">停止</button>"#, i)
            } else {
                format!(r#"<button type="submit" name="action" value="start_{}" class="btn-sm btn-start">启动</button>"#, i)
            };
            
            rows_html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class='{}'>{}</td><td>{}</td></tr>",
                i, desc, status_class, status_text, action_btn
            ));
        }

        let html = format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>FrpcStartup 控制台</title>
    <style>
        body {{ font-family: "Microsoft YaHei", sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; background: #f0f2f5; margin: 0; }}
        .card {{ background: white; padding: 2rem; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); text-align: center; width: 800px; }}
        .message {{ color: blue; margin-bottom: 15px; font-weight: bold; }}
        
        table {{ width: 100%; border-collapse: collapse; margin-bottom: 20px; text-align: left; }}
        th, td {{ padding: 12px; border-bottom: 1px solid #eee; }}
        th {{ background-color: #f8f9fa; font-weight: 600; }}
        
        .status-running {{ color: #28a745; font-weight: bold; }}
        .status-stopped {{ color: #dc3545; font-weight: bold; }}
        
        input[type="password"] {{ width: 100%; padding: 10px; margin-bottom: 20px; box-sizing: border-box; border: 1px solid #ccc; border-radius: 4px; }}
        
        button {{ cursor: pointer; border: none; border-radius: 4px; transition: background 0.2s; }}
        .btn-sm {{ padding: 6px 12px; font-size: 0.9rem; }}
        .btn-lg {{ padding: 10px 20px; font-size: 1rem; width: 48%; margin: 1%; }}
        
        .btn-start {{ background: #28a745; color: white; }}
        .btn-start:hover {{ background: #218838; }}
        
        .btn-stop {{ background: #dc3545; color: white; }}
        .btn-stop:hover {{ background: #c82333; }}
        
        .global-actions {{ margin-top: 20px; border-top: 1px solid #eee; padding-top: 20px; }}
    </style>
</head>
<body>
    <div class="card">
        <h1>Frpc 控制面板</h1>
        <div class="message">{}</div>
        
        <form method="POST" action="/action">
            <input type="password" name="secret" placeholder="在此输入访问密钥以执行操作" required>
            
            <table>
                <thead>
                    <tr>
                        <th width="50">ID</th>
                        <th>描述</th>
                        <th width="100">状态</th>
                        <th width="100">操作</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
            
            <div class="global-actions">
                <button type="submit" name="action" value="start_all" class="btn-lg btn-start">全部启动</button>
                <button type="submit" name="action" value="stop_all" class="btn-lg btn-stop">全部停止</button>
            </div>
        </form>
    </div>
</body>
</html>
"#,
            message, rows_html
        );

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes());
    }
}
