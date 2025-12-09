use crate::config::{Config, FrpcConfig};
use crate::email::send_email;
use crate::logger::write_app_log;
use chrono::Local;
use std::fs;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct FrpcManager {
    config: Config,
    processes: Vec<Option<Child>>,
    monitor_handles: Vec<thread::JoinHandle<()>>,
    running: Arc<Mutex<bool>>,
}

impl FrpcManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            processes: Vec::new(),
            monitor_handles: Vec::new(),
            running: Arc::new(Mutex::new(false)),
        }
    }

    pub fn start_services(&mut self) {
        let mut running = self.running.lock().unwrap();
        if *running {
            write_app_log("Services are already running.");
            return;
        }

        write_app_log("Starting all frpc services...");
        self.processes.clear();
        self.monitor_handles.clear();

        for (i, frpc_conf) in self.config.frpc.iter().enumerate() {
            match self.spawn_frpc(i, frpc_conf) {
                Ok(child) => {
                    self.processes.push(Some(child));
                    write_app_log(&format!("Started frpc process {} - {}", i, frpc_conf.description));
                }
                Err(e) => {
                    write_app_log(&format!("Failed to start frpc process {} - {}: {}", i, frpc_conf.description, e));
                    self.processes.push(None);
                }
            }
        }

        *running = true;
        
        // Send notification
        let _ = send_email(&self.config, "FrpcStartup 服务已启动", "所有配置的 frpc 服务已尝试启动。", false);
    }

    pub fn stop_services(&mut self) {
        let mut running = self.running.lock().unwrap();
        if !*running {
            write_app_log("Services are not running.");
            return;
        }

        write_app_log("Stopping all frpc services...");
        for (i, child_opt) in self.processes.iter_mut().enumerate() {
            if let Some(child) = child_opt {
                write_app_log(&format!("Killing frpc process {}", i));
                let _ = child.kill();
                let _ = child.wait();
            }
            *child_opt = None;
        }
        self.processes.clear();
        *running = false;
        
        let _ = send_email(&self.config, "FrpcStartup 服务已停止", "所有 frpc 服务已停止。", false);
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    fn spawn_frpc(&self, index: usize, frpc_config: &FrpcConfig) -> Result<Child, Box<dyn std::error::Error>> {
        let now = Local::now();
        let log_dir = "logs";
        fs::create_dir_all(log_dir)?;

        let log_file_path = format!(
            "{}/frpc_{}_{}.log",
            log_dir,
            index,
            now.format("%Y-%m-%d_%H-%M-%S")
        );
        let log_file = fs::File::create(&log_file_path)?;

        let stdout_log = log_file.try_clone()?;
        let stderr_log = log_file.try_clone()?;

        let child = Command::new(r"frpc.exe")
            .arg("-f")
            .arg(&frpc_config.arg)
            .stdout(stdout_log)
            .stderr(stderr_log)
            .spawn()?;

        Ok(child)
    }
    
    // 简单的健康检查，如果发现进程挂了，可以重启或者报警（这里简化为只记录日志）
    pub fn check_health(&mut self) {
        let running = *self.running.lock().unwrap();
        if !running {
            return;
        }

        for (i, child_opt) in self.processes.iter_mut().enumerate() {
            if let Some(child) = child_opt {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        write_app_log(&format!("Frpc process {} exited unexpectedly with status: {:?}", i, status));
                        // 这里可以添加重启逻辑
                        *child_opt = None; 
                        
                        // 发送报警邮件
                         let _ = send_email(&self.config, "FrpcStartup 进程异常退出", &format!("进程 {} 异常退出: {:?}", i, status), true);
                    }
                    Ok(None) => {
                        // Still running
                    }
                    Err(e) => {
                        write_app_log(&format!("Error checking process {} status: {}", i, e));
                    }
                }
            }
        }
    }
}
