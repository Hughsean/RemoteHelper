use std::collections::HashMap;
use std::fs;
use std::path::Path;
use crate::driver::error::{DriverError, DriverResult};
use crate::driver::ioctl::IoctlInterface;

use tracing;

/// Pawn module manager for loading and executing Pawn scripts
pub struct PawnModuleManager {
    ioctl: IoctlInterface,
    loaded_modules: HashMap<String, bool>,
    module_ioctls: HashMap<String, IoctlInterface>,
    modules_path: String,
    pawn_script_supported: bool,
}

impl PawnModuleManager {
    /// Create new Pawn module manager
    pub fn new(ioctl: IoctlInterface, modules_path: &str) -> Self {
        Self {
            ioctl,
            loaded_modules: HashMap::new(),
            module_ioctls: HashMap::new(),
            modules_path: modules_path.to_string(),
            pawn_script_supported: true,
        }
    }

    /// Load a Pawn module by name
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

        // Construct module file path
        let module_path = Path::new(&self.modules_path).join(format!("{}.bin", module_name));

        if !module_path.exists() {
            return Err(DriverError::IoctlError(format!(
                "Module file not found: {:?}", module_path
            )));
        }

        // Read module binary data
        let binary_data = fs::read(&module_path)
            .map_err(|e| DriverError::IoctlError(format!(
                "Failed to read module file {:?}: {}", module_path, e
            )))?;

        tracing::info!("Loading Pawn module: {} ({} bytes)", module_name, binary_data.len());

        // PawnIO associates a loaded module with the handle.
        // Keep a dedicated handle per module, like LibreHardwareMonitor's C# wrappers.
        let module_ioctl = self.ioctl.clone();

        // Load the binary into the driver
        if let Err(e) = module_ioctl.load_pawn_binary(module_name, &binary_data) {
            // Official PawnIO driver may not implement Pawn script IOCTLs.
            if matches!(e, DriverError::NotSupported(_)) {
                self.pawn_script_supported = false;
            }

            self.loaded_modules.insert(module_name.to_string(), false);
            return Err(e);
        }

        self.module_ioctls.insert(module_name.to_string(), module_ioctl);

        // Mark as loaded
        self.loaded_modules.insert(module_name.to_string(), true);

        tracing::info!("Successfully loaded Pawn module: {}", module_name);
        Ok(())
    }

    /// Execute a Pawn function
    pub fn execute_function(&self, module_name: &str, function_name: &str, parameters: &[u64]) -> DriverResult<Vec<u64>> {
        // Ensure module is loaded
        if !self.loaded_modules.get(module_name).unwrap_or(&false) {
            return Err(DriverError::IoctlError(format!(
                "Module {} not loaded", module_name
            )));
        }

        tracing::debug!("Executing Pawn function: {}.{} with {} parameters",
                   module_name, function_name, parameters.len());

        let module_ioctl = self.module_ioctls.get(module_name).ok_or_else(|| {
            DriverError::IoctlError(format!("Module {} not loaded", module_name))
        })?;

        let result = module_ioctl.execute_pawn_function(function_name, parameters)?;

        tracing::debug!("Pawn function {}.{} returned: {:?}", module_name, function_name, result);
        Ok(result)
    }

    /// Execute a Pawn function with automatic module loading
    pub fn call_function(&mut self, module_name: &str, function_name: &str, args: &[u64]) -> DriverResult<Vec<u64>> {
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
    /// Read MSR register using AMDFamily17 module
    pub fn read_msr_amd(&mut self, register: u32) -> DriverResult<u64> {
        // Prefer Pawn module when available; fallback to direct IOCTL if script interface is unsupported.
        if self.pawn_script_supported {
            if let Ok(result) = self.call_function("AMDFamily17", "ioctl_read_msr", &[register as u64]) {
                return Ok(result[0]);
            }
        }

        self.ioctl.read_msr(register)
    }

    /// Read SMN register using AMDFamily17 module
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
        if self.pawn_script_supported {
            if let Ok(result) = self.call_function("RyzenSMU", "ReadSmuRegister", &[address as u64]) {
                return Ok(result[0] as u32);
            }
        }

        self.ioctl.read_smu_register(address)
    }

    /// Write SMU register using RyzenSMU module
    pub fn write_smu_register(&mut self, address: u32, value: u32) -> DriverResult<()> {
        if self.pawn_script_supported {
            if self
                .call_function("RyzenSMU", "WriteSmuRegister", &[address as u64, value as u64])
                .is_ok()
            {
                return Ok(());
            }
        }

        self.ioctl.write_smu_register(address, value)
    }

    /// Send SMU command using RyzenSMU module
    pub fn send_smu_command(&mut self, command: u32, address: u32, data: u32) -> DriverResult<u32> {
        if self.pawn_script_supported {
            if let Ok(result) = self.call_function(
                "RyzenSMU",
                "SendSmuCommand",
                &[command as u64, address as u64, data as u64],
            ) {
                return Ok(result[0] as u32);
            }
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










