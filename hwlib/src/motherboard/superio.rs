use crate::driver::PawnModuleManager;

#[derive(Debug, Clone)]
pub struct SuperIoChip {
    pub name: String,
}

/// 通过 LPC I/O (index/data 端口 0x2E/0x2F 或 0x4E/0x4F) 来探测常见的 SuperIO 芯片。
///
/// 实现基于常用的进入配置模式 (0x87,0x87)，读取芯片 ID/Revision 的策略。
pub fn detect_superio(pm: &mut PawnModuleManager) -> Option<SuperIoChip> {
    // Reuse the diagnostic function and return the first detected chip
    if let Some(chip) = diagnose(pm).into_iter().find(|s| s.starts_with("Detected SuperIO")) {
        // Parse a simple name from the diagnostic line
        // Format: "Detected SuperIO chip: <name> (details: [...])"
        if let Some(rest) = chip.splitn(4, ':').nth(1) {
            let name = rest.trim().to_string();
            return Some(SuperIoChip { name });
        }
    }
    None
}

/// Perform diagnostic reads/writes and return a list of human-readable diagnostic lines.
pub fn diagnose(pm: &mut PawnModuleManager) -> Vec<String> {
    let mut out = Vec::new();
    const INDEX_PORTS: &[(u16, u16)] = &[(0x2E, 0x2F), (0x4E, 0x4F)];
    const ENTER_SEQS: &[(u8, u8)] = &[(0x87, 0x87), (0x55, 0x55), (0x55, 0xAA)];
    const ID_REGS: &[u8] = &[0x20, 0x21, 0x23, 0x25, 0xA0];

    out.push("Starting SuperIO diagnostic via LPC ports".to_string());

    for &(idx, data_port) in INDEX_PORTS {
        out.push(format!("Trying LPC index=0x{:X}, data=0x{:X}", idx, data_port));

        // Try simple reads first
        match pm.read_port_byte(idx) {
            Ok(v) => out.push(format!("Read index port 0x{:X} => 0x{:02X}", idx, v)),
            Err(e) => out.push(format!("Read index port 0x{:X} failed: {}", idx, e)),
        }
        match pm.read_port_byte(data_port) {
            Ok(v) => out.push(format!("Read data port 0x{:X} => 0x{:02X}", data_port, v)),
            Err(e) => out.push(format!("Read data port 0x{:X} failed: {}", data_port, e)),
        }

        for &(a, b) in ENTER_SEQS {
            out.push(format!("Attempting enter sequence: 0x{:02X},0x{:02X}", a, b));

            match pm.write_port_byte(idx, a) {
                Ok(()) => out.push(format!("WritePortByte(idx=0x{:X}, {:#02X}) ok", idx, a)),
                Err(e) => {
                    out.push(format!("WritePortByte(idx=0x{:X}, {:#02X}) failed: {}", idx, a, e));
                    continue;
                }
            }

            match pm.write_port_byte(idx, b) {
                Ok(()) => out.push(format!("WritePortByte(idx=0x{:X}, {:#02X}) ok", idx, b)),
                Err(e) => {
                    out.push(format!("WritePortByte(idx=0x{:X}, {:#02X}) failed: {} (exit)", idx, b, e));
                    let _ = pm.write_port_byte(idx, 0xAA);
                    continue;
                }
            }

            let mut id_values = Vec::new();
            for &reg in ID_REGS {
                match pm.write_port_byte(idx, reg) {
                    Ok(()) => out.push(format!("Index write reg=0x{:02X} ok", reg)),
                    Err(e) => {
                        out.push(format!("Index write reg=0x{:02X} failed: {}", reg, e));
                        continue;
                    }
                }
                match pm.read_port_byte(data_port) {
                    Ok(v) => {
                        out.push(format!("Read reg 0x{:02X} => 0x{:02X}", reg, v));
                        id_values.push((reg, v));
                    }
                    Err(e) => out.push(format!("Read reg 0x{:02X} failed: {}", reg, e)),
                }
            }

            let _ = pm.write_port_byte(idx, 0xAA);

            let meaningful: Vec<(u8, u8)> = id_values
                .iter()
                .cloned()
                .filter(|&(_r, v)| v != 0x00 && v != 0xFF)
                .collect();

            if !meaningful.is_empty() {
                out.push(format!("Detected SuperIO chip: {:?} at idx=0x{:X}", meaningful, idx));
                return out;
            }
        }
    }

    out.push("No SuperIO detected via LPC ports".to_string());
    out
}
