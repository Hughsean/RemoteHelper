use crate::config::{Config, FrpcConfig};
// use crate::email::send_email;
use crate::logger::write_app_log;
use chrono::Local;
use std::fs;
use std::process::{Child, Command};

pub struct FrpcManager {
    config: Config,
    processes: Vec<Option<Child>>,
}

impl FrpcManager {
    pub fn new(config: Config) -> Self {
        let count = config.frpc.len();
        let mut processes = Vec::with_capacity(count);
        for _ in 0..count {
            processes.push(None);
        }
        Self { config, processes }
    }

    pub fn start_all(&mut self) {
        write_app_log("Starting all frpc services...");
        for i in 0..self.config.frpc.len() {
            self.start_service(i);
        }
        // Send notification
        // let _ = send_email(
        //     &self.config,
        //     "FrpcStartup 服务已启动",
        //     "所有配置的 frpc 服务已尝试启动。",
        //     false,
        // );
    }

    pub fn stop_all(&mut self) {
        write_app_log("Stopping all frpc services...");
        for i in 0..self.config.frpc.len() {
            self.stop_service(i);
        }
        // let _ = send_email(
        //     &self.config,
        //     "FrpcStartup 服务已停止",
        //     "所有 frpc 服务已停止。",
        //     false,
        // );
    }

    pub fn start_service(&mut self, index: usize) {
        if index >= self.processes.len() {
            return;
        }

        // Check if already running
        if let Some(child) = &mut self.processes[index] {
            if let Ok(None) = child.try_wait() {
                // Still running
                return;
            }
        }

        let frpc_conf = &self.config.frpc[index];
        match self.spawn_frpc(index, frpc_conf) {
            Ok(child) => {
                self.processes[index] = Some(child);
                write_app_log(&format!(
                    "Started frpc process {} - {}",
                    index, frpc_conf.description
                ));
            }
            Err(e) => {
                write_app_log(&format!(
                    "Failed to start frpc process {} - {}: {}",
                    index, frpc_conf.description, e
                ));
            }
        }
    }

    pub fn stop_service(&mut self, index: usize) {
        if index >= self.processes.len() {
            return;
        }

        if let Some(child) = &mut self.processes[index] {
            write_app_log(&format!("Stopping frpc process {}", index));
            let _ = child.kill();
            let _ = child.wait();
        }
        self.processes[index] = None;
    }

    pub fn get_all_statuses(&mut self) -> Vec<(String, bool)> {
        let mut statuses = Vec::new();
        for (i, conf) in self.config.frpc.iter().enumerate() {
            let is_running = if let Some(child) = &mut self.processes[i] {
                match child.try_wait() {
                    Ok(None) => true,
                    _ => false,
                }
            } else {
                false
            };
            statuses.push((conf.description.clone(), is_running));
        }
        statuses
    }

    #[allow(dead_code)]
    pub fn is_any_running(&mut self) -> bool {
        for child_opt in &mut self.processes {
            if let Some(child) = child_opt {
                if let Ok(None) = child.try_wait() {
                    return true;
                }
            }
        }
        false
    }

    fn spawn_frpc(
        &self,
        index: usize,
        frpc_config: &FrpcConfig,
    ) -> Result<Child, Box<dyn std::error::Error>> {
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
        for (i, child_opt) in self.processes.iter_mut().enumerate() {
            if let Some(child) = child_opt {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        write_app_log(&format!(
                            "Frpc process {} exited unexpectedly with status: {:?}",
                            i, status
                        ));
                        // 这里可以添加重启逻辑
                        *child_opt = None;

                        // 发送报警邮件
                        // let _ = send_email(
                        //     &self.config,
                        //     "FrpcStartup 进程异常退出",
                        //     &format!("进程 {} 异常退出: {:?}", i, status),
                        //     true,
                        // );
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
