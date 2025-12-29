use crate::config::WebControlConfig;
use crate::frpc::ServiceManager;
use crate::logger::write_app_log;
use std::collections::HashMap;
use std::fs;
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
    service_manager: Arc<Mutex<ServiceManager>>,
    tunnel_process: Option<Child>,
    ip_tracker: Arc<Mutex<HashMap<String, IpStatus>>>,
}

impl WebServer {
    pub fn new(config: WebControlConfig, service_manager: Arc<Mutex<ServiceManager>>) -> Self {
        Self {
            config,
            service_manager,
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

                    match self.read_full_request(&mut stream) {
                        Ok(request) if !request.is_empty() => {
                            self.handle_request(&request, &mut stream, &ip);
                        }
                        Ok(_) => {}
                        Err(e) => {
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

    fn read_full_request(&self, stream: &mut TcpStream) -> std::io::Result<String> {
        let mut buffer = Vec::new();
        let mut temp_buf = [0u8; 1024];

        loop {
            match stream.read(&mut temp_buf) {
                Ok(n) => {
                    if n == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&temp_buf[..n]);

                    if buffer.len() > MAX_REQUEST_SIZE {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Request too large",
                        ));
                    }

                    // Check if we have headers end
                    if let Some(headers_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        let body_start = headers_end + 4;

                        // Parse Content-Length from headers
                        let headers_bytes = &buffer[..headers_end];
                        let headers_str = String::from_utf8_lossy(headers_bytes);

                        let mut content_length = 0;
                        for line in headers_str.lines() {
                            if line.to_lowercase().starts_with("content-length:") {
                                if let Some(val) = line.split(':').nth(1) {
                                    content_length = val.trim().parse::<usize>().unwrap_or(0);
                                }
                                break;
                            }
                        }

                        if buffer.len() >= body_start + content_length {
                            // We have the full body
                            return Ok(String::from_utf8_lossy(
                                &buffer[..body_start + content_length],
                            )
                            .to_string());
                        }
                    }
                }
                Err(e) => {
                    if buffer.is_empty() && e.kind() == std::io::ErrorKind::TimedOut {
                        // Timeout on first read, likely a pre-connect or idle connection.
                        return Ok(String::new());
                    }
                    return Err(e);
                }
            }
        }

        // If we get here (EOF), return what we have
        Ok(String::from_utf8_lossy(&buffer).to_string())
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
        let body = if let Some(idx) = request.find("\r\n\r\n") {
            &request[idx + 4..]
        } else {
            ""
        };

        if method == "GET" && path == "/" {
            self.serve_dashboard(stream);
        } else if method == "POST" && path == "/api/action" {
            self.handle_api_action(body, stream, client_ip);
        } else {
            let response = "HTTP/1.1 404 Not Found\r\n\r\n";
            let _ = stream.write_all(response.as_bytes());
        }
    }

    fn handle_api_action(&self, body: &str, stream: &mut TcpStream, client_ip: &str) {
        // Parse body: action=start&secret=xxx
        let params: HashMap<String, String> = body
            .split('&')
            .filter_map(|s| {
                let mut split = s.splitn(2, '=');
                if let (Some(k), Some(v)) = (split.next(), split.next()) {
                    Some((k.to_string(), url_decode(v)))
                } else {
                    None
                }
            })
            .collect();

        let secret = params.get("secret").map(|s| s.as_str()).unwrap_or("");
        let action = params.get("action").map(|s| s.as_str()).unwrap_or("");

        let mut success = false;
        let mut _message = String::new();

        if secret != self.config.auth_secret {
            self.record_failed_attempt(client_ip);
            write_app_log(&format!(
                "Invalid secret attempt from IP: {}; secret={}",
                client_ip, secret
            ));
            // 故意延迟响应，防止计时攻击
            std::thread::sleep(Duration::from_millis(1000));
            _message = "密钥错误！".to_string();
        } else {
            // 验证成功，重置计数器
            self.reset_failed_attempts(client_ip);
            success = true;

            let mut mgr = self.service_manager.lock().unwrap();

            if action == "start_all" {
                mgr.start_all_controllable();
                _message = "可控服务已启动".to_string();
            } else if action == "stop_all" {
                mgr.stop_all_controllable();
                _message = "可控服务已停止".to_string();
            } else if let Some(idx_str) = action.strip_prefix("start_") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if mgr.is_web_control_allowed(idx) {
                        mgr.start_service(idx);
                        _message = format!("服务 #{} 已启动", idx);
                    } else {
                        success = false;
                        _message = "该服务未开放 Web 控制".to_string();
                    }
                } else {
                    success = false;
                    _message = "无效的服务索引".to_string();
                }
            } else if let Some(idx_str) = action.strip_prefix("stop_") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if mgr.is_web_control_allowed(idx) {
                        mgr.stop_service(idx);
                        _message = format!("服务 #{} 已停止", idx);
                    } else {
                        success = false;
                        _message = "该服务未开放 Web 控制".to_string();
                    }
                } else {
                    success = false;
                    _message = "无效的服务索引".to_string();
                }
            } else {
                success = false;
                _message = "未知操作".to_string();
            }
        }

        let response_json = format!(r#"{{"success": {}, "message": "{}"}}"#, success, _message);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            response_json.len(),
            response_json
        );
        let _ = stream.write_all(response.as_bytes());
    }

    fn serve_dashboard(&self, stream: &mut TcpStream) {
        let mut mgr = self.service_manager.lock().unwrap();
        let statuses = mgr.get_all_statuses();

        let mut rows_html = String::new();
        for (i, (desc, is_running, allow_web)) in statuses.iter().enumerate() {
            let status_class = if *is_running {
                "status-running"
            } else {
                "status-stopped"
            };
            let status_text = if *is_running {
                "运行中"
            } else {
                "已停止"
            };
            let action_btn = if !*allow_web {
                "<span style='color:#888;'>禁用</span>".to_string()
            } else if *is_running {
                format!(
                    r#"<button onclick="sendAction('stop_{}')" class="btn-sm btn-stop">停止</button>"#,
                    i
                )
            } else {
                format!(
                    r#"<button onclick="sendAction('start_{}')" class="btn-sm btn-start">启动</button>"#,
                    i
                )
            };

            rows_html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class='{}'>{}</td><td>{}</td></tr>",
                i, desc, status_class, status_text, action_btn
            ));
        }

        let html = render_dashboard_html(&rows_html);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(response.as_bytes());
    }
}

fn render_dashboard_html(rows_html: &str) -> String {
    const TEMPLATE_PATH: &str = "web/dashboard.html";
    match fs::read_to_string(TEMPLATE_PATH) {
        Ok(tpl) => tpl.replace("{{ROWS}}", rows_html),
        Err(e) => {
            write_app_log(&format!(
                "Failed to read dashboard template ({}): {}. Falling back to built-in HTML.",
                TEMPLATE_PATH, e
            ));
            fallback_dashboard_html(rows_html)
        }
    }
}

fn fallback_dashboard_html(rows_html: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>FrpcStartup 服务控制台</title>
    <style>
        :root {{
            --bg: #0f172a;
            --card: rgba(255, 255, 255, 0.06);
            --card-border: rgba(255, 255, 255, 0.12);
            --text: #e2e8f0;
            --muted: #94a3b8;
            --primary: #10b981;
            --primary-strong: #0f9b6b;
            --danger: #ef4444;
            --danger-strong: #dc2626;
            --table-stripe: rgba(255, 255, 255, 0.04);
            --shadow: 0 20px 60px rgba(0,0,0,0.35);
        }}

        body {{ font-family: "Microsoft YaHei", "Segoe UI", sans-serif; background: radial-gradient(circle at 10% 20%, rgba(16,185,129,0.12), transparent 35%), radial-gradient(circle at 90% 10%, rgba(59,130,246,0.1), transparent 30%), var(--bg); color: var(--text); margin: 0; min-height: 100vh; display: flex; justify-content: center; align-items: center; padding: 24px; box-sizing: border-box; }}

        .card {{ width: 940px; max-width: 100%; background: var(--card); border: 1px solid var(--card-border); border-radius: 16px; padding: 28px; box-shadow: var(--shadow); backdrop-filter: blur(10px); }}

        .header {{ display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 18px; }}
        .title {{ font-size: 22px; letter-spacing: 0.4px; }}
        .subtitle {{ color: var(--muted); font-size: 14px; }}

        .input-row {{ margin-bottom: 16px; }}
        .input-row label {{ display: block; color: var(--muted); margin-bottom: 6px; font-size: 14px; }}
        input[type="password"] {{ width: 100%; padding: 12px 14px; border-radius: 10px; border: 1px solid var(--card-border); background: rgba(255, 255, 255, 0.05); color: var(--text); outline: none; transition: border-color 0.2s ease, background 0.2s ease; }}
        input[type="password"]:focus {{ border-color: var(--primary); background: rgba(255, 255, 255, 0.08); }}

        table {{ width: 100%; border-collapse: collapse; margin-top: 10px; }}
        th, td {{ padding: 12px; text-align: left; }}
        th {{ color: var(--muted); font-weight: 600; font-size: 13px; border-bottom: 1px solid var(--card-border); }}
        tbody tr {{ border-bottom: 1px solid var(--card-border); background: transparent; }}
        tbody tr:nth-child(odd) {{ background: var(--table-stripe); }}
        tbody tr:hover {{ background: rgba(255, 255, 255, 0.08); }}

        .badge {{ display: inline-block; padding: 4px 10px; border-radius: 999px; font-size: 12px; font-weight: 600; }}
        .badge.running {{ background: rgba(16, 185, 129, 0.16); color: #34d399; }}
        .badge.stopped {{ background: rgba(239, 68, 68, 0.16); color: #f87171; }}

        button {{ cursor: pointer; border: none; border-radius: 10px; transition: transform 0.1s ease, box-shadow 0.2s ease, background 0.2s ease; color: #fff; font-weight: 600; }}
        .btn-sm {{ padding: 8px 14px; font-size: 13px; }}
        .btn-lg {{ padding: 12px 18px; font-size: 14px; min-width: 140px; }}
        .btn-start {{ background: linear-gradient(135deg, var(--primary), var(--primary-strong)); box-shadow: 0 10px 30px rgba(16,185,129,0.35); }}
        .btn-start:hover {{ transform: translateY(-1px); box-shadow: 0 12px 34px rgba(16,185,129,0.45); }}
        .btn-stop {{ background: linear-gradient(135deg, var(--danger), var(--danger-strong)); box-shadow: 0 10px 30px rgba(239,68,68,0.35); }}
        .btn-stop:hover {{ transform: translateY(-1px); box-shadow: 0 12px 34px rgba(239,68,68,0.45); }}
        .btn-disabled {{ background: rgba(148,163,184,0.3); color: var(--muted); cursor: not-allowed; box-shadow: none; }}

        .global-actions {{ margin-top: 18px; border-top: 1px solid var(--card-border); padding-top: 18px; display: flex; gap: 12px; flex-wrap: wrap; }}

        .toast {{ position: fixed; top: 16px; left: 50%; transform: translateX(-50%); background: #1f2937; color: #fff; padding: 12px 18px; border-radius: 10px; box-shadow: 0 12px 32px rgba(0,0,0,0.25); opacity: 0; pointer-events: none; transition: opacity 0.2s ease, transform 0.2s ease; z-index: 999; border: 1px solid rgba(255,255,255,0.08); }}
        .toast.show {{ opacity: 1; transform: translateX(-50%) translateY(2px); }}
        .toast.error {{ background: #7f1d1d; }}
    </style>
    <script>
        let toastTimer;

        function showToast(msg, isError) {{
            const toast = document.getElementById('toast');
            toast.textContent = msg;
            toast.classList.toggle('error', !!isError);
            toast.classList.add('show');
            clearTimeout(toastTimer);
            toastTimer = setTimeout(() => toast.classList.remove('show'), 1800);
        }}

        window.onload = function() {{
            const savedSecret = localStorage.getItem('service_secret');
            if (savedSecret) {{
                document.getElementById('secret').value = savedSecret;
            }}
        }};

        async function sendAction(action) {{
            const secret = document.getElementById('secret').value;
            if (!secret) {{
                showToast('请输入访问密钥', true);
                return;
            }}
            
            try {{
                const response = await fetch('/api/action', {{
                    method: 'POST',
                    headers: {{
                        'Content-Type': 'application/x-www-form-urlencoded'
                    }},
                    body: 'action=' + encodeURIComponent(action) + '&secret=' + encodeURIComponent(secret)
                }});
                
                const result = await response.json();
                if (result.success) {{
                    localStorage.setItem('service_secret', secret);
                    showToast(result.message, false);
                    setTimeout(() => location.reload(), 500);
                }} else {{
                    showToast('错误: ' + result.message, true);
                }}
            }} catch (e) {{
                showToast('请求失败: ' + e, true);
            }}
        }}
    </script>
</head>
<body>
    <div id="toast" class="toast"></div>
    <div class="card">
        <div class="header">
            <div class="title">服务控制面板</div>
            <div class="subtitle">FrpcStartup</div>
        </div>

        <div class="input-row">
            <label for="secret">访问密钥</label>
            <input type="password" id="secret" placeholder="在此输入访问密钥以执行操作" required>
        </div>

        <table>
            <thead>
                <tr>
                    <th width="50">ID</th>
                    <th>描述</th>
                    <th width="120">状态</th>
                    <th width="100">操作</th>
                </tr>
            </thead>
            <tbody>
                {rows}
            </tbody>
        </table>
        
        <div class="global-actions">
            <button onclick="sendAction('start_all')" class="btn-lg btn-start">全部启动</button>
            <button onclick="sendAction('stop_all')" class="btn-lg btn-stop">全部停止</button>
        </div>
    </div>
</body>
</html>
"#,
        rows = rows_html
    )
}

fn url_decode(input: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(b) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                bytes.push(b);
            } else {
                bytes.push(b'%');
                bytes.push(h1 as u8);
                bytes.push(h2 as u8);
            }
        } else if c == '+' {
            bytes.push(b' ');
        } else {
            bytes.push(c as u8);
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}
