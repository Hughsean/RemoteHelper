use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::winnt::SERVICE_DEMAND_START;
use winapi::um::winsvc::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, QueryServiceStatus, SC_HANDLE, SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS,
    SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_STATUS, SERVICE_STOPPED, StartServiceW,
};

// 定义可能不可用的服务常量
use tracing;

const SERVICE_KERNEL_DRIVER: u32 = 0x00000001;
use crate::driver::error::{DriverError, DriverResult};

/// Windows service manager for driver loading
pub struct DriverService {
    service_name: String,
    service_handle: Option<SC_HANDLE>,
    scm_handle: Option<SC_HANDLE>,
    driver_path: String,
}

// SC_HANDLE is thread-safe (Windows kernel handles are process-wide)
unsafe impl Send for DriverService {}
unsafe impl Sync for DriverService {}

impl DriverService {
    /// Create new driver service
    pub fn new(driver_path: impl AsRef<Path>, service_name: &str) -> DriverResult<Self> {
        let driver_path = driver_path.as_ref().to_string_lossy().into_owned();
        let service_name = service_name.to_string();

        Ok(Self {
            service_name,
            service_handle: None,
            scm_handle: None,
            driver_path,
        })
    }

    /// Start the driver service
    pub fn start(&mut self) -> DriverResult<()> {
        tracing::info!("Starting driver service: {}", self.service_name);

        // 打开服务控制管理器
        let scm_handle = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_ALL_ACCESS) };

        if scm_handle.is_null() {
            let error_code = unsafe { GetLastError() };
            return Err(DriverError::ServiceError(format!(
                "Failed to open Service Control Manager: error {}",
                error_code
            )));
        }

        self.scm_handle = Some(scm_handle);

        // 先尝试打开现有服务
        let service_name_wide = to_wide_string(&self.service_name);
        let mut service_handle =
            unsafe { OpenServiceW(scm_handle, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS) };

        // 如果服务不存在，则创建它
        if service_handle.is_null() {
            tracing::info!("Service does not exist, creating new service");

            let driver_path_wide = to_wide_string(&self.driver_path);
            let display_name = format!("{} Driver", self.service_name);
            let display_name_wide = to_wide_string(&display_name);

            service_handle = unsafe {
                CreateServiceW(
                    scm_handle,
                    service_name_wide.as_ptr(),
                    display_name_wide.as_ptr(),
                    SERVICE_ALL_ACCESS,
                    SERVICE_KERNEL_DRIVER,
                    SERVICE_DEMAND_START,
                    1, // SERVICE_ERROR_NORMAL
                    driver_path_wide.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                )
            };

            if service_handle.is_null() {
                let error_code = unsafe { GetLastError() };
                return Err(DriverError::ServiceError(format!(
                    "Failed to create service: error {}",
                    error_code
                )));
            }
        }

        self.service_handle = Some(service_handle);

        // 检查当前服务状态
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        let status_result = unsafe { QueryServiceStatus(service_handle, &mut status) };

        if status_result == 0 {
            let error_code = unsafe { GetLastError() };
            return Err(DriverError::ServiceError(format!(
                "Failed to query service status: error {}",
                error_code
            )));
        }

        // 如果尚未运行，则启动服务
        if status.dwCurrentState != SERVICE_RUNNING {
            tracing::info!("Starting service...");

            let start_result = unsafe { StartServiceW(service_handle, 0, ptr::null_mut()) };

            if start_result == 0 {
                let error_code = unsafe { GetLastError() };
                return Err(DriverError::ServiceError(format!(
                    "Failed to start service: error {}",
                    error_code
                )));
            }

            // 等待服务启动（带超时）
            self.wait_for_service_state(SERVICE_RUNNING, 10000)?;
        }

        tracing::info!("Driver service started successfully");
        Ok(())
    }

    /// Stop the driver service
    pub fn stop(&mut self) -> DriverResult<()> {
        tracing::info!("Stopping driver service: {}", self.service_name);

        if let Some(service_handle) = self.service_handle {
            let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };

            let stop_result =
                unsafe { ControlService(service_handle, SERVICE_CONTROL_STOP, &mut status) };

            if stop_result != 0 || status.dwCurrentState == SERVICE_STOPPED {
                // 等待服务停止
                if let Err(e) = self.wait_for_service_state(SERVICE_STOPPED, 5000) {
                    tracing::warn!("Service stop timeout: {}", e);
                }
            }

            // 删除服务
            let delete_result = unsafe { DeleteService(service_handle) };
            if delete_result == 0 {
                let error_code = unsafe { GetLastError() };
                tracing::warn!("Failed to delete service: error {}", error_code);
            }
        }

        self.cleanup_handles();
        tracing::info!("Driver service stopped successfully");
        Ok(())
    }

    /// Wait for service to reach specified state
    fn wait_for_service_state(&self, target_state: u32, timeout_ms: u32) -> DriverResult<()> {
        if let Some(service_handle) = self.service_handle {
            let start_time = std::time::Instant::now();

            loop {
                let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
                let query_result = unsafe { QueryServiceStatus(service_handle, &mut status) };

                if query_result == 0 {
                    let error_code = unsafe { GetLastError() };
                    return Err(DriverError::ServiceError(format!(
                        "Failed to query service status during wait: error {}",
                        error_code
                    )));
                }

                if status.dwCurrentState == target_state {
                    return Ok(());
                }

                if start_time.elapsed().as_millis() > timeout_ms as u128 {
                    return Err(DriverError::Timeout);
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        Err(DriverError::ServiceError(
            "No service handle found".to_string(),
        ))
    }

    /// Check if service is running
    pub fn is_running(&self) -> bool {
        if let Some(service_handle) = self.service_handle {
            let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
            let result = unsafe { QueryServiceStatus(service_handle, &mut status) };

            return result != 0 && status.dwCurrentState == SERVICE_RUNNING;
        }
        false
    }

    /// Cleanup handles
    fn cleanup_handles(&mut self) {
        if let Some(service_handle) = self.service_handle.take() {
            unsafe {
                CloseServiceHandle(service_handle);
            }
        }

        if let Some(scm_handle) = self.scm_handle.take() {
            unsafe {
                CloseServiceHandle(scm_handle);
            }
        }
    }
}

impl Drop for DriverService {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::error!("Failed to stop service during drop: {}", e);
        }
        self.cleanup_handles();
    }
}

/// 将字符串转换为宽字符串（UTF-16）
fn to_wide_string(s: &str) -> Vec<u16> {
    OsString::from(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wide_string_conversion() {
        let test_str = "TestService";
        let wide = to_wide_string(test_str);

        // 应非空且以空字符结尾
        assert!(!wide.is_empty());
        assert_eq!(wide[wide.len() - 1], 0);
    }

    #[test]
    fn test_service_creation() {
        let service = DriverService::new(r"C:\test\driver.sys", "TestDriver");
        assert!(service.is_ok());

        let service = service.unwrap();
        assert_eq!(service.service_name, "TestDriver");
        assert!(service.driver_path.contains("driver.sys"));
    }
}
