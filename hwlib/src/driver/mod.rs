//! Windows 内核驱动接口模块。
//!
//! 为 PawnIO 内核驱动提供安全的 Rust 接口以实现低级硬件访问。
//!
//! ## 功能特性
//!
//! - 驱动服务管理（安装/启动/停止）
//! - 与内核驱动的 IOCTL 通信
//! - MSR（模型特定寄存器）读写
//! - I/O 端口访问
//! - AMD CPU 的 SMU（系统管理单元）命令
//! - Pawn 脚本模块加载和执行
//!
//! ## 安全
//!
//! 所有操作都需要管理员权限。驱动通过 RAII 模式自动
//! 提取、加载和清理。

use std::sync::{Arc, Mutex};
pub mod driver_resource;
pub mod error;
pub mod ioctl;
pub mod pawn_module;
pub mod service_manager;

use tracing;

// Re-export main types
pub use driver_resource::DriverResource;
pub use error::{DriverError, DriverResult};
pub use ioctl::{IoctlInterface, MsrRequest, PortRequest, SmuRequest};
pub use pawn_module::PawnModuleManager;
pub use service_manager::DriverService;

/// PawnIO 内核驱动接口。
///
/// 管理 PawnIO 内核驱动的生命周期，并提供
/// 低级硬件访问的方法。
///
/// # 示例
///
/// ```no_run
/// use hwlib::driver::PawnIoDriver;
///
/// let mut driver = PawnIoDriver::new()?;
/// driver.start()?;
///
/// // 读取 MSR 寄存器
/// let value = driver.read_msr(0xC0011020)?;
///
/// // 清理在 drop 时自动进行
/// # Ok::<(), hwlib::driver::DriverError>(())
/// ```
pub struct PawnIoDriver {
    resource: DriverResource,
    service: Option<DriverService>,
    ioctl: Option<IoctlInterface>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl PawnIoDriver {
    /// Create new PawnIO driver instance
    pub fn new() -> DriverResult<Self> {
        let resource = DriverResource::new()?;
        Ok(Self {
            resource,
            service: None,
            ioctl: None,
            pawn_manager: None,
        })
    }

    /// Initialize and start the driver
    pub fn start(&mut self) -> DriverResult<()> {
        tracing::info!("Starting PawnIO driver...");

        // Extract driver binary to temp location
        // 注意：文件必须具有 .sys 扩展名，Windows 才能将其加载为驱动程序
        let driver_path = self.resource.extract_driver()?;

        tracing::debug!("Driver path: {:?}", driver_path);

        // Create and start Windows service
        let mut service = DriverService::new(&driver_path, "LhmPawnIo")?;
        service.start()?;

        // Initialize IOCTL interface
        let ioctl = IoctlInterface::new(r"\\.\LhmPawnIo")?;

        // Initialize Pawn module manager with path to modules
        let modules_path = resolve_modules_path();
        let pawn_manager = Arc::new(Mutex::new(PawnModuleManager::new(
            ioctl.try_clone()?,
            &modules_path,
        )));

        self.service = Some(service);
        self.ioctl = Some(ioctl);
        self.pawn_manager = Some(pawn_manager);

        tracing::info!("PawnIO driver started successfully");
        Ok(())
    }

    /// Stop the driver and cleanup
    pub fn stop(&mut self) -> DriverResult<()> {
        tracing::info!("Stopping PawnIO driver...");

        // Close Pawn module manager
        self.pawn_manager = None;

        // Close IOCTL interface
        self.ioctl = None;

        // Stop and remove service
        if let Some(mut service) = self.service.take() {
            service.stop()?;
        }

        // Cleanup temporary files
        self.resource.cleanup()?;

        tracing::info!("PawnIO driver stopped successfully");
        Ok(())
    }

    /// Get IOCTL interface reference
    pub fn ioctl(&self) -> DriverResult<&IoctlInterface> {
        self.ioctl
            .as_ref()
            .ok_or_else(|| DriverError::NotInitialized("Driver not started".to_string()))
    }

    /// Get Pawn module manager reference
    pub fn pawn_manager(&self) -> DriverResult<&Arc<Mutex<PawnModuleManager>>> {
        self.pawn_manager
            .as_ref()
            .ok_or_else(|| DriverError::NotInitialized("Driver not started".to_string()))
    }

    /// Read MSR register
    pub fn read_msr(&self, register: u32) -> DriverResult<u64> {
        let ioctl = self.ioctl()?;
        ioctl.read_msr(register)
    }

    /// Read I/O port byte
    pub fn read_port_byte(&self, port: u16) -> DriverResult<u8> {
        let ioctl = self.ioctl()?;
        ioctl.read_port_byte(port)
    }

    /// Write I/O port byte
    pub fn write_port_byte(&self, port: u16, value: u8) -> DriverResult<()> {
        let ioctl = self.ioctl()?;
        ioctl.write_port_byte(port, value)
    }

    /// Send SMU command (PawnIO 0.2.1 feature)
    pub fn send_smu_command(&self, command: u32, address: u32, data: u64) -> DriverResult<u64> {
        let ioctl = self.ioctl()?;
        ioctl.send_smu_command(command, address, data)
    }

    /// Read SMU register (PawnIO 0.2.1 feature)
    pub fn read_smu_register(&self, address: u32) -> DriverResult<u32> {
        let ioctl = self.ioctl()?;
        ioctl.read_smu_register(address)
    }

    /// Write SMU register (PawnIO 0.2.1 feature)
    pub fn write_smu_register(&self, address: u32, value: u32) -> DriverResult<()> {
        let ioctl = self.ioctl()?;
        ioctl.write_smu_register(address, value)
    }
}

impl Drop for PawnIoDriver {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::error!("Failed to stop driver during drop: {}", e);
        }
    }
}

/// Default driver instance for easy access.
///
/// Wrapped in a Mutex to allow re-initialization after failure,
/// unlike `OnceLock` which can never be reset.
use std::sync::OnceLock;
static DEFAULT_DRIVER: OnceLock<PawnIoDriver> = OnceLock::new();

/// Resolve the directory containing Pawn module (.bin) files.
///
/// Looks relative to the executable first; falls back to CWD.
fn resolve_modules_path() -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let candidate = exe_dir.join("PawnIO");
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    // Fallback: current working directory
    "PawnIO".to_string()
}

/// Initialize default driver instance
pub fn init_driver() -> DriverResult<()> {
    // If already initialized successfully, return immediately
    if DEFAULT_DRIVER
        .get()
        .and_then(|d| d.ioctl.as_ref())
        .is_some()
    {
        return Ok(());
    }

    let modules_path = resolve_modules_path();
    tracing::info!("Resolved PawnIO modules path: {}", modules_path);

    // First, try to connect to official PawnIO driver if it's installed
    if check_official_pawnio() {
        tracing::info!("Official PawnIO driver detected, attempting to connect...");

        match connect_official_pawnio() {
            Ok(ioctl) => {
                let pawn_manager = Arc::new(Mutex::new(PawnModuleManager::new(
                    ioctl.try_clone()?,
                    &modules_path,
                )));

                // Capability probe: many installed "PawnIO" devices don't implement the Pawn script IOCTLs.
                let probe = {
                    let mut pm = pawn_manager.lock().unwrap();
                    pm.load_module("AMDFamily17")
                };

                if matches!(probe, Err(DriverError::NotSupported(_))) {
                    tracing::warn!(
                        "Connected to official PawnIO device, but it does not support Pawn script modules; falling back to embedded driver"
                    );
                } else {
                    let resource = DriverResource::new()?;
                    let driver = PawnIoDriver {
                        resource,
                        service: None,
                        ioctl: Some(ioctl),
                        pawn_manager: Some(pawn_manager),
                    };

                    match DEFAULT_DRIVER.set(driver) {
                        Ok(()) => {
                            tracing::info!("Successfully connected to official PawnIO driver");
                            return Ok(());
                        }
                        Err(_) => {
                            tracing::warn!("DEFAULT_DRIVER already set; a previous init succeeded");
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Official PawnIO driver is running but connection failed: {}",
                    e
                );
                tracing::info!("Falling back to embedded driver...");
            }
        }
    } else {
        tracing::info!("Official PawnIO driver not detected, using embedded driver");
    }

    // Fallback: use embedded driver
    let mut driver = PawnIoDriver::new()?;
    driver.start()?;

    match DEFAULT_DRIVER.set(driver) {
        Ok(()) => Ok(()),
        Err(_) => {
            tracing::warn!("DEFAULT_DRIVER already set; a previous init succeeded");
            Ok(())
        }
    }
}

/// Check if official PawnIO driver is installed and running
pub fn check_official_pawnio() -> bool {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::winsvc::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
        SERVICE_QUERY_STATUS,
    };

    unsafe {
        // Open Service Control Manager
        let scm_handle = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);

        if scm_handle.is_null() {
            tracing::debug!("Failed to open Service Control Manager");
            return false;
        }

        // Try to open PawnIO service
        let service_name: Vec<u16> = OsString::from("PawnIO")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let service_handle = OpenServiceW(scm_handle, service_name.as_ptr(), SERVICE_QUERY_STATUS);

        if service_handle.is_null() {
            tracing::debug!("PawnIO service not found");
            CloseServiceHandle(scm_handle);
            return false;
        }

        // Query service status
        let mut status = std::mem::zeroed();
        let result = QueryServiceStatus(service_handle, &mut status);

        let is_running =
            result != 0 && status.dwCurrentState == winapi::um::winsvc::SERVICE_RUNNING;

        tracing::debug!(
            "PawnIO service query result: {}, state: {}",
            result,
            status.dwCurrentState
        );

        CloseServiceHandle(service_handle);
        CloseServiceHandle(scm_handle);

        is_running
    }
}

/// Try to connect to official PawnIO driver
pub fn connect_official_pawnio() -> DriverResult<IoctlInterface> {
    tracing::info!("Attempting to connect to official PawnIO driver...");

    // Official PawnIO uses "\\.\PawnIO" as device name
    match IoctlInterface::new(r"\\.\PawnIO") {
        Ok(ioctl) => {
            tracing::info!("Successfully connected to official PawnIO driver");
            Ok(ioctl)
        }
        Err(e) => {
            tracing::warn!("Failed to connect to official PawnIO driver: {}", e);
            Err(e)
        }
    }
}

/// Get default driver instance
pub fn get_driver() -> DriverResult<&'static PawnIoDriver> {
    DEFAULT_DRIVER
        .get()
        .ok_or_else(|| DriverError::NotInitialized("Driver not initialized".to_string()))
}

/// Convenience functions for Pawn module operations
pub mod pawn {
    use super::*;

    /// Read MSR register using AMDFamily17 module
    pub fn read_msr_amd(pawn_manager: &mut PawnModuleManager, register: u32) -> DriverResult<u64> {
        pawn_manager.read_msr_amd(register)
    }

    /// Read SMU register using RyzenSMU module
    pub fn read_smu_register(
        pawn_manager: &mut PawnModuleManager,
        address: u32,
    ) -> DriverResult<u32> {
        pawn_manager.read_smu_register(address)
    }

    /// Write SMU register using RyzenSMU module
    pub fn write_smu_register(
        pawn_manager: &mut PawnModuleManager,
        address: u32,
        value: u32,
    ) -> DriverResult<()> {
        pawn_manager.write_smu_register(address, value)
    }

    /// Send SMU command using RyzenSMU module
    pub fn send_smu_command(
        pawn_manager: &mut PawnModuleManager,
        command: u32,
        address: u32,
        data: u32,
    ) -> DriverResult<u32> {
        pawn_manager.send_smu_command(command, address, data)
    }

    /// Read I/O port byte using LpcIO module
    pub fn read_port_byte(pawn_manager: &mut PawnModuleManager, port: u16) -> DriverResult<u8> {
        pawn_manager.read_port_byte(port)
    }

    /// Write I/O port byte using LpcIO module
    pub fn write_port_byte(
        pawn_manager: &mut PawnModuleManager,
        port: u16,
        value: u8,
    ) -> DriverResult<()> {
        pawn_manager.write_port_byte(port, value)
    }
}
