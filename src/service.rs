use crate::config::{Config, ServiceConfig};
// use crate::email::send_email;
use crate::logger::write_app_log;
use chrono::Local;
use std::fs;
use std::process::{Child, Command};

pub struct ServiceManager {
    config: Config,
    processes: Vec<Option<Child>>,
}

impl ServiceManager {
    pub fn new(config: Config) -> Self {
        let count = config.service.len();
        let mut processes = Vec::with_capacity(count);
        for _ in 0..count {
            processes.push(None);
        }
        Self { config, processes }
    }

    pub fn start_all(&mut self) {
        write_app_log("Starting all configured services...");
        for i in 0..self.config.service.len() {
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
        write_app_log("Stopping all configured services...");
        for i in 0..self.config.service.len() {
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

        let svc_conf = &self.config.service[index];
        match self.spawn_service(index, svc_conf) {
            Ok(child) => {
                self.processes[index] = Some(child);
                write_app_log(&format!(
                    "Started service {} - {}",
                    index, svc_conf.description
                ));
            }
            Err(e) => {
                write_app_log(&format!(
                    "Failed to start service {} - {}: {}",
                    index, svc_conf.description, e
                ));
            }
        }
    }

    pub fn stop_service(&mut self, index: usize) {
        if index >= self.processes.len() {
            return;
        }

        if let Some(child) = &mut self.processes[index] {
            write_app_log(&format!("Stopping service {}", index));
            let _ = child.kill();
            let _ = child.wait();
        }
        self.processes[index] = None;
    }

    pub fn get_all_statuses(&mut self) -> Vec<(String, bool, bool)> {
        self.config
            .service
            .iter()
            .enumerate()
            .map(|(i, conf)| {
                let is_running = if let Some(child) = &mut self.processes[i] {
                    matches!(child.try_wait(), Ok(None))
                } else {
                    false
                };
                let allow_web = conf.allow_web_control.unwrap_or(true);
                (conf.description.clone(), is_running, allow_web)
            })
            .collect()
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

    pub fn start_auto_services(&mut self) {
        write_app_log("Starting auto services...");
        let indices: Vec<usize> = self
            .config
            .service
            .iter()
            .enumerate()
            .filter_map(|(i, svc)| svc.auto_start.unwrap_or(false).then_some(i))
            .collect();
        for i in indices {
            self.start_service(i);
        }
    }

    pub fn start_all_controllable(&mut self) {
        for i in 0..self.config.service.len() {
            if self.is_web_control_allowed(i) {
                self.start_service(i);
            }
        }
    }

    pub fn stop_all_controllable(&mut self) {
        for i in 0..self.config.service.len() {
            if self.is_web_control_allowed(i) {
                self.stop_service(i);
            }
        }
    }

    pub fn is_web_control_allowed(&self, index: usize) -> bool {
        self.config
            .service
            .get(index)
            .and_then(|svc| svc.allow_web_control)
            .unwrap_or(true)
    }

    fn spawn_service(
        &self,
        index: usize,
        service_config: &ServiceConfig,
    ) -> Result<Child, Box<dyn std::error::Error>> {
        let now = Local::now();
        let log_dir = "logs";
        fs::create_dir_all(log_dir)?;

        let log_file_path = format!(
            "{}/service_{}_{}.log",
            log_dir,
            index,
            now.format("%Y-%m-%d_%H-%M-%S")
        );
        let log_file = fs::File::create(&log_file_path)?;

        let stdout_log = log_file.try_clone()?;
        let stderr_log = log_file.try_clone()?;

        let exe_path = service_config
            .exe_path
            .clone()
            .unwrap_or_else(|| "frpc.exe".to_string());
        let args = service_config.args.clone().unwrap_or_default();

        let mut cmd = Command::new(exe_path);
        for arg in args {
            cmd.arg(arg);
        }

        if let Some(dir) = &service_config.working_dir {
            cmd.current_dir(dir);
        }

        // 记录启动命令，便于排查未监听端口或路径错误的问题
        write_app_log(&format!(
            "Spawning service {}: exe={:?}, args={:?}, workdir={:?}",
            index,
            cmd.get_program(),
            cmd.get_args().collect::<Vec<_>>(),
            cmd.get_current_dir()
        ));

        let child = cmd.stdout(stdout_log).stderr(stderr_log).spawn()?;

        Ok(child)
    }

    // 简单的健康检查，如果发现进程挂了，可以重启或者报警（这里简化为只记录日志）
    pub fn check_health(&mut self) {
        for (i, child_opt) in self.processes.iter_mut().enumerate() {
            if let Some(child) = child_opt {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        write_app_log(&format!(
                            "Service {} exited unexpectedly with status: {:?}",
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
