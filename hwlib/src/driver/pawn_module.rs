use crate::driver::error::{DriverError, DriverResult};
use crate::driver::ioctl::IoctlInterface;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

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
    ioctl: Arc<IoctlInterface>,
    loaded_modules: HashMap<String, bool>,
    modules_path: String,
    pawn_script_supported: bool,
    /// 驱动中当前驻留的模块名。
    ///
    /// PawnIO 驱动一次只保留一个模块 — 加载新模块会驱逐旧模块。
    /// 此字段追踪哪个模块在驱动里，以便切换时自动重载。
    current_module: Option<String>,
}

impl PawnModuleManager {
    /// 创建新的 Pawn 模块管理器。
    ///
    /// 接收已打开的 `Arc<IoctlInterface>`，以便与调用方共享同一个内核驱动句柄。
    /// C# LibreHardwareMonitor 在同一个 handle 上执行 LoadBinary 和 Execute，
    /// 创建新 handle 可能导致 Execute 返回 ERROR_INVALID_PARAMETER。
    pub fn new(ioctl: Arc<IoctlInterface>, modules_path: &str) -> Self {
        Self {
            ioctl,
            loaded_modules: HashMap::new(),
            modules_path: modules_path.to_string(),
            pawn_script_supported: true,
            current_module: None,
        }
    }

    /// 读取模块二进制数据（磁盘或嵌入回退）。
    fn read_module_binary(&self, module_name: &str) -> DriverResult<Vec<u8>> {
        let module_path = Path::new(&self.modules_path).join(format!("{}.bin", module_name));

        if module_path.exists() {
            match fs::read(&module_path) {
                Ok(b) => return Ok(b),
                Err(e) => {
                    tracing::warn!(
                        "Failed to read module file {:?}: {}, falling back to embedded",
                        module_path,
                        e
                    );
                }
            }
        }

        if let Some(embedded) = crate::driver::driver_resource::embedded_module_bytes(module_name) {
            tracing::info!(
                "Module file {:?} not found, using embedded binary",
                module_path
            );
            Ok(embedded.to_vec())
        } else {
            Err(DriverError::IoctlError(format!(
                "Module file not found: {:?}",
                module_path
            )))
        }
    }

    /// 从磁盘加载 Pawn 模块到驱动。
    ///
    /// 注意：PawnIO 驱动一次只保留一个模块。加载新模块会驱逐旧模块，
    /// 因此 [`call_function`] 在调用前会自动重载正确的模块。
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

        let binary_data = self.read_module_binary(module_name)?;

        tracing::info!(
            "Loading Pawn module: {} ({} bytes)",
            module_name,
            binary_data.len()
        );

        // 发送二进制到驱动
        if let Err(e) = self.ioctl.load_pawn_binary(module_name, &binary_data) {
            // 官方的 PawnIO 驱动可能未实现 Pawn 脚本的 IOCTL
            if matches!(e, DriverError::NotSupported(_)) {
                self.pawn_script_supported = false;
            }

            self.loaded_modules.insert(module_name.to_string(), false);
            return Err(e);
        }

        // 标记为已加载 + 追踪驱动中的当前模块
        self.loaded_modules.insert(module_name.to_string(), true);
        self.current_module = Some(module_name.to_string());

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

        let result = self
            .ioctl
            .execute_pawn_function(function_name, parameters)?;

        tracing::debug!(
            "Pawn function {}.{} returned {} values",
            module_name,
            function_name,
            result.len()
        );
        Ok(result)
    }

    /// Execute a Pawn function with an explicit output size in 64-bit cells.
    pub fn execute_function_with_output_len(
        &self,
        module_name: &str,
        function_name: &str,
        parameters: &[u64],
        output_values: usize,
    ) -> DriverResult<Vec<u64>> {
        if !self.loaded_modules.get(module_name).unwrap_or(&false) {
            return Err(DriverError::IoctlError(format!(
                "Module {} not loaded",
                module_name
            )));
        }

        tracing::debug!(
            "Executing Pawn function: {}.{} with {} parameters, {} output values",
            module_name,
            function_name,
            parameters.len(),
            output_values
        );

        let result = self.ioctl.execute_pawn_function_with_output_len(
            function_name,
            parameters,
            output_values,
        )?;

        tracing::debug!(
            "Pawn function {}.{} returned {} values",
            module_name,
            function_name,
            result.len()
        );
        Ok(result)
    }

    /// 确保指定模块当前驻留在驱动中（必要时重载）。
    ///
    /// PawnIO 驱动一次只保留一个模块 — 先前的模块被驱逐后必须重载。
    fn ensure_driver_module(&mut self, module_name: &str) -> DriverResult<()> {
        if self.current_module.as_deref() == Some(module_name) {
            return Ok(());
        }

        tracing::debug!(
            "Switching Pawn module: {} -> {}",
            self.current_module.as_deref().unwrap_or("none"),
            module_name
        );

        let binary = self.read_module_binary(module_name)?;
        self.ioctl.load_pawn_binary(module_name, &binary)?;
        self.current_module = Some(module_name.to_string());
        Ok(())
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
        // 确保模块二进制已知（首次加载时录入 loaded_modules）
        if !self.loaded_modules.contains_key(module_name) {
            self.load_module(module_name)?;
        }
        // 驱动一次只存一个模块 — 必要时重载
        self.ensure_driver_module(module_name)?;

        self.execute_function(module_name, function_name, args)
    }

    /// Execute a Pawn function with automatic module loading and explicit output size.
    pub fn call_function_with_output_len(
        &mut self,
        module_name: &str,
        function_name: &str,
        args: &[u64],
        output_values: usize,
    ) -> DriverResult<Vec<u64>> {
        if !self.pawn_script_supported {
            return Err(DriverError::NotSupported(
                "Pawn script interface is not supported by the connected driver".to_string(),
            ));
        }
        if !self.loaded_modules.contains_key(module_name) {
            self.load_module(module_name)?;
        }
        self.ensure_driver_module(module_name)?;

        self.execute_function_with_output_len(module_name, function_name, args, output_values)
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
        if self.pawn_script_supported {
            match self.call_function_with_output_len(
                "AMDFamily17",
                "ioctl_read_msr",
                &[register as u64],
                1,
            ) {
                Ok(result) if !result.is_empty() => {
                    tracing::debug!("Pawn MSR read 0x{:X} = 0x{:X}", register, result[0]);
                    return Ok(result[0]);
                }
                Ok(_) => {
                    tracing::debug!(
                        "Pawn MSR read 0x{:X}: empty result, treating as failure",
                        register
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Pawn MSR read failed for 0x{:X}: {} — falling back to direct IOCTL",
                        register,
                        e
                    );
                }
            }
        }

        match self.ioctl.read_msr(register) {
            Ok(val) => {
                tracing::debug!("Direct IOCTL MSR read 0x{:X} = 0x{:X}", register, val);
                Ok(val)
            }
            Err(e) => {
                tracing::warn!("Direct IOCTL MSR read 0x{:X} also failed: {}", register, e);
                Err(e)
            }
        }
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
            match self.call_function_with_output_len(
                "AMDFamily17",
                "ioctl_read_smn",
                &[offset as u64],
                1,
            ) {
                Ok(result) if !result.is_empty() => return Ok(result[0] as u32),
                Ok(_) => {
                    tracing::debug!(
                        "Pawn SMN read 0x{:X}: empty result, treating as failure",
                        offset
                    );
                    return Err(DriverError::InvalidResponse(format!(
                        "Pawn SMN read 0x{:X} returned no values",
                        offset
                    )));
                }
                Err(e) => {
                    tracing::warn!("Pawn SMN read failed for 0x{:X}: {}", offset, e);
                    return Err(e);
                }
            }
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
