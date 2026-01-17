use crate::driver::error::{DriverError, DriverResult};
use crate::driver::ioctl::IoctlInterface;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use tracing;

/// Pawn 脚本模块管理器。
///
/// 管理 Pawn 字节码模块的加载和执行，以实现扩展的
/// 硬件访问功能。
///
/// ## Pawn 模块
///
/// - **AMDFamily17**: AMD Zen CPU MSR/SMN 访问
/// - **RyzenSMU**: AMD SMU（系统管理单元）命令
/// - **IsaBridgeEC**: 嵌入式控制器访问
///
/// ## 示例
///
/// ```no_run
/// use hwlib::driver::{PawnModuleManager, IoctlInterface};
///
/// let ioctl = IoctlInterface::new(r"\\.\\LhmPawnIo")?;
/// let mut manager = PawnModuleManager::new(ioctl, "PawnIO");
///
/// // 读取 AMD SMN 寄存器
/// let value = manager.read_smn_amd(0x00059800)?;
/// # Ok::<(), hwlib::driver::DriverError>(())
/// ```
pub struct PawnModuleManager {
    ioctl: IoctlInterface,
    loaded_modules: HashMap<String, bool>,
    module_ioctls: HashMap<String, IoctlInterface>,
    modules_path: String,
    pawn_script_supported: bool,
}

impl PawnModuleManager {
    /// 创建新的 Pawn 模块管理器。
    ///
    /// # 参数
    ///
    /// * `ioctl` - 到内核驱动的 IOCTL 接口
    /// * `modules_path` - 包含 .bin 模块文件的目录
    pub fn new(ioctl: IoctlInterface, modules_path: &str) -> Self {
        Self {
            ioctl,
            loaded_modules: HashMap::new(),
            module_ioctls: HashMap::new(),
            modules_path: modules_path.to_string(),
            pawn_script_supported: true,
        }
    }

    /// 从磁盘加载 Pawn 模块。
    ///
    /// 模块会被缓存 - 使用相同名称的后续调用是空操作。
    ///
    /// # 参数
    ///
    /// * `module_name` - 模块名称（不含 .bin 扩展名）
    ///
    /// # 错误
    ///
    /// - [`DriverError::IoctlError`] 如果文件未找到或加载失败
    /// - [`DriverError::NotSupported`] 如果驱动不支持 Pawn 脚本
    pub fn load_module(&mut self, module_name: &str) -> DriverResult<()> {
        if self.loaded_modules.contains_key(module_name) {
            tracing::debug!("Module {} already loaded", module_name);
            return Ok(());
        }

        if !self.pawn_script_supported {
            self.loaded_modules.insert(module_name.to_string(), false);
            return Err(DriverError::NotSupported(
                "Pawn script interface is not supported by the connected driver".to_string(),
            ));
        }

        // 构造模块文件路径
        let module_path = Path::new(&self.modules_path).join(format!("{}.bin", module_name));

        // 尝试从磁盘读取模块二进制；如果不存在或读取失败，回退到嵌入数据（如果可用）
        let binary_data: Vec<u8> = if module_path.exists() {
            match fs::read(&module_path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        "Failed to read module file {:?}: {}, falling back to embedded",
                        module_path,
                        e
                    );
                    if let Some(embedded) =
                        crate::driver::driver_resource::embedded_module_bytes(module_name)
                    {
                        embedded.to_vec()
                    } else {
                        return Err(DriverError::IoctlError(format!(
                            "Failed to read module file {:?}: {}",
                            module_path, e
                        )));
                    }
                }
            }
        } else if let Some(embedded) =
            crate::driver::driver_resource::embedded_module_bytes(module_name)
        {
            tracing::info!(
                "Module file {:?} not found, using embedded binary",
                module_path
            );
            embedded.to_vec()
        } else {
            return Err(DriverError::IoctlError(format!(
                "Module file not found: {:?}",
                module_path
            )));
        };

        tracing::info!(
            "Loading Pawn module: {} ({} bytes)",
            module_name,
            binary_data.len()
        );

        // PawnIO 将已加载的模块与句柄关联。
        // 为每个模块保留专用句柄，类似 LibreHardwareMonitor 的 C# 封装。
        let module_ioctl = self.ioctl.clone();

        // 将二进制加载到驱动中
        if let Err(e) = module_ioctl.load_pawn_binary(module_name, &binary_data) {
            // 官方的 PawnIO 驱动可能未实现 Pawn 脚本的 IOCTL。
            if matches!(e, DriverError::NotSupported(_)) {
                self.pawn_script_supported = false;
            }

            self.loaded_modules.insert(module_name.to_string(), false);
            return Err(e);
        }

        self.module_ioctls
            .insert(module_name.to_string(), module_ioctl);

        // 标记为已加载
        self.loaded_modules.insert(module_name.to_string(), true);

        tracing::info!("Successfully loaded Pawn module: {}", module_name);
        Ok(())
    }

    /// Execute a Pawn function
    pub fn execute_function(
        &self,
        module_name: &str,
        function_name: &str,
        parameters: &[u64],
    ) -> DriverResult<Vec<u64>> {
        // Ensure module is loaded
        if !self.loaded_modules.get(module_name).unwrap_or(&false) {
            return Err(DriverError::IoctlError(format!(
                "Module {} not loaded",
                module_name
            )));
        }

        tracing::debug!(
            "Executing Pawn function: {}.{} with {} parameters",
            module_name,
            function_name,
            parameters.len()
        );

        let module_ioctl = self
            .module_ioctls
            .get(module_name)
            .ok_or_else(|| DriverError::IoctlError(format!("Module {} not loaded", module_name)))?;

        let result = module_ioctl.execute_pawn_function(function_name, parameters)?;

        tracing::debug!(
            "Pawn function {}.{} returned: {:?}",
            module_name,
            function_name,
            result
        );
        Ok(result)
    }

    /// Execute a Pawn function with automatic module loading
    pub fn call_function(
        &mut self,
        module_name: &str,
        function_name: &str,
        args: &[u64],
    ) -> DriverResult<Vec<u64>> {
        if !self.pawn_script_supported {
            return Err(DriverError::NotSupported(
                "Pawn script interface is not supported by the connected driver".to_string(),
            ));
        }
        // Load module if not already loaded
        if !self.loaded_modules.contains_key(module_name) {
            self.load_module(module_name)?;
        }

        self.execute_function(module_name, function_name, args)
    }

    /// Check if a module is loaded
    pub fn is_module_loaded(&self, module_name: &str) -> bool {
        *self.loaded_modules.get(module_name).unwrap_or(&false)
    }

    /// Get list of loaded modules
    pub fn loaded_modules(&self) -> Vec<String> {
        self.loaded_modules.keys().cloned().collect()
    }

    /// Get modules path
    pub fn modules_path(&self) -> &str {
        &self.modules_path
    }

    /// Whether the connected driver supports Pawn script IOCTLs
    pub fn pawn_script_supported(&self) -> bool {
        self.pawn_script_supported
    }
}

/// Convenience functions for common Pawn module operations
impl PawnModuleManager {
    /// 在 AMD CPU 上读取 MSR 寄存器。
    ///
    /// 优先使用 AMDFamily17 Pawn 模块（如果可用），
    /// 如果 Pawn 脚本不受支持则回退到直接 IOCTL。
    ///
    /// # 参数
    ///
    /// * `register` - MSR 寄存器地址（例如，0xC0011020）
    ///
    /// # 错误
    ///
    /// 如果读取失败或驱动未初始化则返回 [`DriverError`]。
    pub fn read_msr_amd(&mut self, register: u32) -> DriverResult<u64> {
        // Prefer Pawn module when available; fallback to direct IOCTL if script interface is unsupported.
        if self.pawn_script_supported
            && let Ok(result) =
                self.call_function("AMDFamily17", "ioctl_read_msr", &[register as u64])
        {
            return Ok(result[0]);
        }

        self.ioctl.read_msr(register)
    }

    /// 在 AMD CPU 上读取 SMN（系统管理网络）寄存器。
    ///
    /// 需要 AMDFamily17 Pawn 模块。
    ///
    /// # 参数
    ///
    /// * `offset` - SMN 寄存器偏移量（例如，0x00059800 用于温度）
    ///
    /// # 错误
    ///
    /// 如果 Pawn 脚本不可用则返回 [`DriverError::NotSupported`]。
    pub fn read_smn_amd(&mut self, offset: u32) -> DriverResult<u32> {
        if self.pawn_script_supported {
            let result = self.call_function("AMDFamily17", "ioctl_read_smn", &[offset as u64])?;
            return Ok(result[0] as u32);
        }

        Err(DriverError::NotSupported(
            "SMN read requires Pawn script module support".to_string(),
        ))
    }

    /// Read SMU register using RyzenSMU module
    pub fn read_smu_register(&mut self, address: u32) -> DriverResult<u32> {
        if self.pawn_script_supported
            && let Ok(result) = self.call_function("RyzenSMU", "ReadSmuRegister", &[address as u64])
        {
            return Ok(result[0] as u32);
        }

        self.ioctl.read_smu_register(address)
    }

    /// Write SMU register using RyzenSMU module
    pub fn write_smu_register(&mut self, address: u32, value: u32) -> DriverResult<()> {
        if self.pawn_script_supported
            && self
                .call_function(
                    "RyzenSMU",
                    "WriteSmuRegister",
                    &[address as u64, value as u64],
                )
                .is_ok()
        {
            return Ok(());
        }

        self.ioctl.write_smu_register(address, value)
    }

    /// Send SMU command using RyzenSMU module
    pub fn send_smu_command(&mut self, command: u32, address: u32, data: u32) -> DriverResult<u32> {
        if self.pawn_script_supported
            && let Ok(result) = self.call_function(
                "RyzenSMU",
                "SendSmuCommand",
                &[command as u64, address as u64, data as u64],
            )
        {
            return Ok(result[0] as u32);
        }

        Ok(self.ioctl.send_smu_command(command, address, data as u64)? as u32)
    }

    /// Read I/O port byte using LpcIO module
    pub fn read_port_byte(&mut self, port: u16) -> DriverResult<u8> {
        let result = self.call_function("LpcIO", "ReadPortByte", &[port as u64])?;
        Ok(result[0] as u8)
    }

    /// Write I/O port byte using LpcIO module
    pub fn write_port_byte(&mut self, port: u16, value: u8) -> DriverResult<()> {
        self.call_function("LpcIO", "WritePortByte", &[port as u64, value as u64])?;
        Ok(())
    }
}
