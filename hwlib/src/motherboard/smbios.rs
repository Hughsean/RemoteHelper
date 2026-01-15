use tracing;

/// 简单的 SMBIOS 基板（Baseboard）信息占位结构。
#[derive(Debug, Clone)]
pub struct SmbiosInfo {
    pub manufacturer: String,
    pub product: String,
    pub version: Option<String>,
    pub serial: Option<String>,
}

/// 从系统获取基板信息（Windows 实现：使用 `wmic baseboard get ... /format:list`）。
///
/// 备注：`wmic` 在一些系统上可能不可用；若失败则返回 None。
pub fn get_baseboard_info() -> Option<SmbiosInfo> {
    #[cfg(target_os = "windows")]
    {
        use winapi::um::sysinfoapi::GetSystemFirmwareTable;

        // Provider signature for raw SMBIOS table: 'RSMB' (ASCII)
        let provider = u32::from_le_bytes(*b"RSMB");

        // First call to get required buffer size
        let size = unsafe { GetSystemFirmwareTable(provider, 0, std::ptr::null_mut(), 0) } as usize;
        if size == 0 {
            tracing::warn!("GetSystemFirmwareTable returned size 0 for RSMB");
            return None;
        }

        let mut buf = vec![0u8; size];
        let ret = unsafe { GetSystemFirmwareTable(provider, 0, buf.as_mut_ptr() as *mut _, size as u32) };
        if ret == 0 {
            tracing::warn!("GetSystemFirmwareTable failed to retrieve data");
            return None;
        }

        // Parse SMBIOS structures and look for Type 2 (Baseboard)
        let mut i: usize = 0;
        while i + 4 <= buf.len() {
            let stype = buf[i];
            let length = buf[i + 1] as usize;

            if length < 4 || i + length > buf.len() {
                break;
            }

            if stype == 127 {
                // End-of-table
                break;
            }

            // Parse strings following the formatted area
            let mut strings: Vec<String> = Vec::new();
            let mut j = i + length;
            while j < buf.len() {
                if buf[j] == 0 {
                    // check for double 0 terminator
                    if j + 1 < buf.len() && buf[j + 1] == 0 {
                        j += 2;
                        break;
                    } else {
                        // single zero -> empty string
                        strings.push(String::new());
                        j += 1;
                        continue;
                    }
                }

                let start = j;
                while j < buf.len() && buf[j] != 0 {
                    j += 1;
                }
                if j > start {
                    if let Ok(s) = std::str::from_utf8(&buf[start..j]) {
                        strings.push(s.trim().to_string());
                    } else {
                        strings.push(String::new());
                    }
                }
                // skip the terminating zero
                if j < buf.len() && buf[j] == 0 {
                    j += 1;
                }
            }

            if stype == 2 {
                // Baseboard: offsets for strings follow SMBIOS spec
                let get_str = |offset: usize| -> String {
                    if offset < length {
                        let idx = buf[i + offset] as usize;
                        if idx > 0 && idx - 1 < strings.len() {
                            return strings[idx - 1].clone();
                        }
                    }
                    String::new()
                };

                let manufacturer = get_str(0x04);
                let product = get_str(0x05);
                let version = get_str(0x06);
                let serial = get_str(0x07);

                if !manufacturer.is_empty() || !product.is_empty() {
                    tracing::info!("SMBIOS baseboard parsed: {} {}", manufacturer, product);
                    return Some(SmbiosInfo {
                        manufacturer,
                        product,
                        version: if version.is_empty() { None } else { Some(version) },
                        serial: if serial.is_empty() { None } else { Some(serial) },
                    });
                }
            }

            // Move to next structure
            // j now points to the start of next structure (after double 0)
            if j == 0 {
                // safety
                break;
            }
            i = j;
        }

        tracing::debug!("SMBIOS Type 2 not found in RSMB table");
        None
    }

    #[cfg(not(target_os = "windows"))]
    {
        tracing::debug!("SMBIOS baseboard lookup not implemented for non-Windows OS");
        None
    }
}
