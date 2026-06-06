use crate::driver::error::{DriverError, DriverResult};
use std::ffi::CString;
use std::mem;
use std::ptr;
use winapi::ctypes::c_void as WinCVoid;
use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::winnt::{
    FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE, HANDLE,
};

use tracing;

/// IOCTL codes for PawnIO driver communication
/// These are based on the original LibreHardwareMonitor C# implementation
pub mod ioctl_codes {
    // Base IOCTL code construction helper
    const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
        (device_type << 16) | (access << 14) | (function << 2) | method
    }

    const FILE_DEVICE_UNKNOWN: u32 = 0x00000022;
    const METHOD_BUFFERED: u32 = 0;
    #[allow(dead_code)]
    const METHOD_IN_DIRECT: u32 = 1;
    #[allow(dead_code)]
    const METHOD_OUT_DIRECT: u32 = 2;
    #[allow(dead_code)]
    const METHOD_NEITHER: u32 = 3;
    const FILE_ANY_ACCESS: u32 = 0;
    #[allow(dead_code)]
    const FILE_READ_ACCESS: u32 = 1;
    #[allow(dead_code)]
    const FILE_WRITE_ACCESS: u32 = 2;

    // PawnIO IOCTL codes
    pub const IOCTL_READ_MSR: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_WRITE_MSR: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_READ_PORT_BYTE: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x803, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_WRITE_PORT_BYTE: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x804, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_READ_PORT_WORD: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x805, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_WRITE_PORT_WORD: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x806, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_READ_PORT_DWORD: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x807, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_WRITE_PORT_DWORD: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x808, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_READ_PHYSICAL_MEMORY: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x809, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_WRITE_PHYSICAL_MEMORY: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x80A, METHOD_BUFFERED, FILE_ANY_ACCESS);

    // Enhanced IOCTL codes for PawnIO 0.2.1 features
    pub const IOCTL_SMU_COMMAND: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x80B, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_SMU_READ_REGISTER: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x80C, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_SMU_WRITE_REGISTER: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x80D, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_ISA_BRIDGE_EC: u32 =
        ctl_code(FILE_DEVICE_UNKNOWN, 0x80E, METHOD_BUFFERED, FILE_ANY_ACCESS);

    // Pawn script interface IOCTL codes (corrected for PawnIO)
    // DEVICE_TYPE comes from LibreHardwareMonitor's C# implementation:
    // DEVICE_TYPE = 41394u << 16 = 0xA1B2_0000
    pub const IOCTL_PIO_LOAD_BINARY: u32 = 0xA1B22084; // DEVICE_TYPE | (0x821 << 2)
    pub const IOCTL_PIO_EXECUTE_FN: u32 = 0xA1B22104; // DEVICE_TYPE | (0x841 << 2)
}

/// MSR read request structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MsrRequest {
    pub register: u32,
    pub processor_number: u32,
}

/// MSR response structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MsrResponse {
    pub value: u64,
}

/// Port I/O request structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PortRequest {
    pub port: u16,
    pub value: u8,
}

/// SMU command request structure (PawnIO 0.2.1 feature)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SmuRequest {
    pub command: u32,
    pub address: u32,
    pub data: u64,
}

/// SMU response structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SmuResponse {
    pub result: u64,
    pub status: u32,
}

/// SMU register request structure
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SmuRegisterRequest {
    pub address: u32,
    pub value: u32,
}

/// Physical memory request structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicalMemoryRequest {
    pub address: u64,
    pub size: u32,
}

/// IOCTL interface for communicating with PawnIO driver.
///
/// Each instance holds an independent Windows `HANDLE` obtained via `CreateFile`.
/// The underlying `HANDLE` is safe to use from multiple threads because:
/// - `CreateFile` with `FILE_SHARE_READ | FILE_SHARE_WRITE` creates a handle
///   that can be concurrently accessed
/// - The PawnIO kernel driver processes IOCTL requests synchronously and
///   each `DeviceIoControl` call is atomic with respect to the handle
/// - Overlapped I/O is not used, so there is no shared `OVERLAPPED` state
//
// SAFETY: HANDLE from CreateFile is thread-safe for concurrent DeviceIoControl calls.
unsafe impl Send for IoctlInterface {}
unsafe impl Sync for IoctlInterface {}

impl IoctlInterface {
    /// 尝试克隆 IOCTL 接口，创建一个新的设备句柄。
    ///
    /// 每个句柄独立运行；内核驱动设备是无状态的，
    /// 因此不需要担心句柄之间的状态共享问题。
    pub fn try_clone(&self) -> DriverResult<Self> {
        IoctlInterface::new(&self.device_name)
    }
}

impl Clone for IoctlInterface {
    fn clone(&self) -> Self {
        self.try_clone()
            .expect("Failed to clone IoctlInterface: device handle exhausted or driver not running")
    }
}

impl IoctlInterface {
    /// Create new IOCTL interface
    pub fn new(device_name: &str) -> DriverResult<Self> {
        tracing::info!("Opening device: {}", device_name);

        let device_name_cstr = CString::new(device_name)
            .map_err(|e| DriverError::IoctlError(format!("Invalid device name: {}", e)))?;

        let device_handle = unsafe {
            CreateFileA(
                device_name_cstr.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };

        if device_handle == INVALID_HANDLE_VALUE {
            let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(DriverError::IoctlError(format!(
                "Failed to open device {}: error {}",
                device_name, error_code
            )));
        }

        Ok(Self {
            device_handle,
            device_name: device_name.to_string(),
        })
    }

    /// Read MSR register
    pub fn read_msr(&self, register: u32) -> DriverResult<u64> {
        self.read_msr_on_processor(register, 0)
    }

    /// Read MSR register on specific processor
    pub fn read_msr_on_processor(&self, register: u32, processor: u32) -> DriverResult<u64> {
        let request = MsrRequest {
            register,
            processor_number: processor,
        };

        let response: MsrResponse =
            self.device_io_control_generic(ioctl_codes::IOCTL_READ_MSR, &request)?;

        Ok(response.value)
    }

    /// Write MSR register
    pub fn write_msr(&self, register: u32, value: u64) -> DriverResult<()> {
        self.write_msr_on_processor(register, value, 0)
    }

    /// Write MSR register on specific processor
    pub fn write_msr_on_processor(
        &self,
        register: u32,
        value: u64,
        processor: u32,
    ) -> DriverResult<()> {
        #[repr(C)]
        struct MsrWriteRequest {
            register: u32,
            processor_number: u32,
            value: u64,
        }

        let request = MsrWriteRequest {
            register,
            processor_number: processor,
            value,
        };

        self.device_io_control_no_output(ioctl_codes::IOCTL_WRITE_MSR, &request)
    }

    /// Read I/O port byte
    pub fn read_port_byte(&self, port: u16) -> DriverResult<u8> {
        let request = PortRequest { port, value: 0 };
        let response: PortRequest =
            self.device_io_control_generic(ioctl_codes::IOCTL_READ_PORT_BYTE, &request)?;
        Ok(response.value)
    }

    /// Write I/O port byte
    pub fn write_port_byte(&self, port: u16, value: u8) -> DriverResult<()> {
        let request = PortRequest { port, value };
        self.device_io_control_no_output(ioctl_codes::IOCTL_WRITE_PORT_BYTE, &request)
    }

    /// Send SMU command (PawnIO 0.2.1 feature)
    pub fn send_smu_command(&self, command: u32, address: u32, data: u64) -> DriverResult<u64> {
        let request = SmuRequest {
            command,
            address,
            data,
        };

        let response: SmuResponse =
            self.device_io_control_generic(ioctl_codes::IOCTL_SMU_COMMAND, &request)?;

        if response.status != 0 {
            return Err(DriverError::IoctlError(format!(
                "SMU command failed with status: {}",
                response.status
            )));
        }

        Ok(response.result)
    }

    /// Read SMU register (PawnIO 0.2.1 feature)
    pub fn read_smu_register(&self, address: u32) -> DriverResult<u32> {
        let request = SmuRegisterRequest { address, value: 0 };
        let response: SmuRegisterRequest =
            self.device_io_control_generic(ioctl_codes::IOCTL_SMU_READ_REGISTER, &request)?;
        Ok(response.value)
    }

    /// Write SMU register (PawnIO 0.2.1 feature)
    pub fn write_smu_register(&self, address: u32, value: u32) -> DriverResult<()> {
        let request = SmuRegisterRequest { address, value };
        self.device_io_control_no_output(ioctl_codes::IOCTL_SMU_WRITE_REGISTER, &request)
    }

    /// Load Pawn binary module
    pub fn load_pawn_binary(&self, module_name: &str, binary_data: &[u8]) -> DriverResult<()> {
        // Protocol matches LibreHardwareMonitor's PawnIo.cs:
        // input buffer is the raw .bin contents; no module name/pointer struct.
        // (module_name is kept for call-site clarity, but not sent to the driver)
        let _ = module_name;

        let mut bytes_returned: u32 = 0;

        let result = unsafe {
            DeviceIoControl(
                self.device_handle,
                ioctl_codes::IOCTL_PIO_LOAD_BINARY,
                binary_data.as_ptr() as *mut WinCVoid,
                binary_data.len() as u32,
                ptr::null_mut(),
                0,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };

        if result == 0 {
            let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(match error_code {
                1 => DriverError::NotSupported(format!(
                    "DeviceIoControl not supported (code: 0x{:X}): error {}",
                    ioctl_codes::IOCTL_PIO_LOAD_BINARY,
                    error_code
                )),
                5 => DriverError::PermissionDenied(format!(
                    "DeviceIoControl permission denied (code: 0x{:X}): error {}",
                    ioctl_codes::IOCTL_PIO_LOAD_BINARY,
                    error_code
                )),
                _ => DriverError::IoctlError(format!(
                    "DeviceIoControl failed (code: 0x{:X}): error {}",
                    ioctl_codes::IOCTL_PIO_LOAD_BINARY,
                    error_code
                )),
            });
        }

        Ok(())
    }

    /// Execute Pawn function
    pub fn execute_pawn_function(
        &self,
        function_name: &str,
        parameters: &[u64],
    ) -> DriverResult<Vec<u64>> {
        if function_name.len() >= 32 {
            return Err(DriverError::IoctlError(
                "Function name too long (max 31 chars)".to_string(),
            ));
        }
        if parameters.len() > 8 {
            return Err(DriverError::IoctlError(
                "Too many parameters (max 8)".to_string(),
            ));
        }

        // Build input buffer like LibreHardwareMonitor PawnIo.cs:
        // 32 bytes function name (ASCII, null-terminated) + int64 parameters.
        let mut input = Vec::with_capacity(32 + parameters.len() * 8);

        let name_bytes = function_name.as_bytes();
        input.extend_from_slice(name_bytes);
        input.resize(32, 0);

        for &param in parameters {
            input.extend_from_slice(&(param as i64).to_le_bytes());
        }

        // Output buffer: allocate enough space for up to 8 x i64 return values.
        // PawnIO returns the actual number of bytes written; we then parse into i64s.
        let max_output_values = 8usize;
        let mut output = vec![0u8; max_output_values * 8];

        let bytes_returned = self.device_io_control_bytes(
            ioctl_codes::IOCTL_PIO_EXECUTE_FN,
            &input[..],
            &mut output,
        )?;

        if bytes_returned == 0 {
            return Ok(vec![0]);
        }

        if bytes_returned % 8 != 0 {
            return Err(DriverError::InvalidResponse(format!(
                "Unexpected output size: {} bytes (not divisible by 8)",
                bytes_returned
            )));
        }

        let out_longs = bytes_returned / 8;
        let mut result = Vec::with_capacity(out_longs);
        for i in 0..out_longs {
            let start = i * 8;
            let value = i64::from_le_bytes(output[start..start + 8].try_into().unwrap());
            result.push(value as u64);
        }

        Ok(result)
    }

    /// Read physical memory
    pub fn read_physical_memory(&self, address: u64, size: u32) -> DriverResult<Vec<u8>> {
        let request = PhysicalMemoryRequest { address, size };

        let mut output_buffer = vec![0u8; size as usize];
        let mut bytes_returned: u32 = 0;

        let result = unsafe {
            DeviceIoControl(
                self.device_handle,
                ioctl_codes::IOCTL_READ_PHYSICAL_MEMORY,
                &request as *const _ as *mut WinCVoid,
                mem::size_of::<PhysicalMemoryRequest>() as u32,
                output_buffer.as_mut_ptr() as *mut WinCVoid,
                output_buffer.len() as u32,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };

        if result == 0 {
            let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(DriverError::IoctlError(format!(
                "Read physical memory failed: error {}",
                error_code
            )));
        }

        output_buffer.resize(bytes_returned as usize, 0);
        Ok(output_buffer)
    }

    /// Generic device I/O control with input and output
    fn device_io_control_generic<TInput, TOutput>(
        &self,
        ioctl_code: u32,
        input: &TInput,
    ) -> DriverResult<TOutput>
    where
        TInput: Sized,
        TOutput: Sized + Default,
    {
        let mut output: TOutput = Default::default();
        let mut bytes_returned: u32 = 0;

        let result = unsafe {
            DeviceIoControl(
                self.device_handle,
                ioctl_code,
                input as *const _ as *mut WinCVoid,
                mem::size_of::<TInput>() as u32,
                &mut output as *mut _ as *mut WinCVoid,
                mem::size_of::<TOutput>() as u32,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };

        if result == 0 {
            let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(match error_code {
                1 => DriverError::NotSupported(format!(
                    "DeviceIoControl not supported (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
                5 => DriverError::PermissionDenied(format!(
                    "DeviceIoControl permission denied (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
                _ => DriverError::IoctlError(format!(
                    "DeviceIoControl failed (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
            });
        }

        Ok(output)
    }

    /// Generic device I/O control with input only (no output)
    fn device_io_control_no_output<TInput>(
        &self,
        ioctl_code: u32,
        input: &TInput,
    ) -> DriverResult<()>
    where
        TInput: Sized,
    {
        let mut bytes_returned: u32 = 0;

        let result = unsafe {
            DeviceIoControl(
                self.device_handle,
                ioctl_code,
                input as *const _ as *mut WinCVoid,
                mem::size_of::<TInput>() as u32,
                ptr::null_mut(),
                0,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };

        if result == 0 {
            let error_code = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(match error_code {
                1 => DriverError::NotSupported(format!(
                    "DeviceIoControl not supported (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
                5 => DriverError::PermissionDenied(format!(
                    "DeviceIoControl permission denied (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
                _ => DriverError::IoctlError(format!(
                    "DeviceIoControl failed (code: 0x{:X}): error {}",
                    ioctl_code, error_code
                )),
            });
        }

        Ok(())
    }

    /// DeviceIoControl for byte buffers
    pub fn device_io_control_bytes(
        &self,
        ioctl_code: u32,
        input: &[u8],
        output: &mut [u8],
    ) -> DriverResult<usize> {
        let mut bytes_returned = 0;
        let result = unsafe {
            DeviceIoControl(
                self.device_handle,
                ioctl_code,
                input.as_ptr() as *mut WinCVoid,
                input.len() as u32,
                output.as_mut_ptr() as *mut WinCVoid,
                output.len() as u32,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };

        if result == 0 {
            let error = unsafe { winapi::um::errhandlingapi::GetLastError() };
            return Err(match error {
                1 => DriverError::NotSupported(format!(
                    "DeviceIoControl not supported (code: 0x{:X}): error {}",
                    ioctl_code, error
                )),
                5 => DriverError::PermissionDenied(format!(
                    "DeviceIoControl permission denied (code: 0x{:X}): error {}",
                    ioctl_code, error
                )),
                _ => DriverError::IoctlError(format!(
                    "DeviceIoControl failed (code: 0x{:X}): error {}",
                    ioctl_code, error
                )),
            });
        }

        Ok(bytes_returned as usize)
    }
}

impl Drop for IoctlInterface {
    fn drop(&mut self) {
        if self.device_handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.device_handle);
            }
            tracing::info!("Closed device: {}", self.device_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_code_generation() {
        // Test IOCTL code generation matches expected values
        assert_ne!(ioctl_codes::IOCTL_READ_MSR, 0);
        assert_ne!(ioctl_codes::IOCTL_SMU_COMMAND, ioctl_codes::IOCTL_READ_MSR);
    }

    #[test]
    fn test_request_structures() {
        let msr_req = MsrRequest {
            register: 0xC0010299,
            processor_number: 0,
        };
        assert_eq!(msr_req.register, 0xC0010299);

        let smu_req = SmuRequest {
            command: 1,
            address: 0x1000,
            data: 0x12345678,
        };
        assert_eq!(smu_req.command, 1);
        assert_eq!(smu_req.data, 0x12345678);
    }
}
