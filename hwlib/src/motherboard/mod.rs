// 模块声明 - 已实现基础骨架
pub mod ec;
pub mod motherboard;
pub mod smbios;
pub mod superio;
// pub mod config; // config 支持待完善

// 重新导出主要类型
pub use motherboard::Motherboard;
pub use smbios::SmbiosInfo;
pub use superio::SuperIoChip;

use crate::core::Hardware;
use crate::driver::PawnModuleManager;
use std::sync::{Arc, Mutex};

/// 主板组管理器。
///
/// 管理检测到的主板硬件实例，并协调它们的更新与 PawnIO 驱动集成。
pub struct MotherboardGroup {
    motherboards: Vec<Box<dyn crate::core::Hardware>>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl Default for MotherboardGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl MotherboardGroup {
    pub fn new() -> Self {
        Self {
            motherboards: Vec::new(),
            pawn_manager: None,
        }
    }

    /// 设置 PawnIO 驱动管理器以实现低级硬件访问。
    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        tracing::info!("Setting pawn_manager for MotherboardGroup");
        self.pawn_manager = Some(pm);
    }

    /// 检测系统中的主板。
    ///
    /// 当前实现会尝试解析 SMBIOS 获取主板信息，并创建一个通用的 `Motherboard` 实例。
    /// 如果后续实现了 SuperIO/EC 等检测，会在此处扩展。
    pub fn detect_motherboards(&mut self) -> crate::core::HardwareResult<()> {
        tracing::info!("Detecting motherboards...");

        // Try to parse SMBIOS for board info
        let board_name = match smbios::get_baseboard_info() {
            Some(info) => {
                tracing::info!(
                    "Detected motherboard: {} {}",
                    info.manufacturer,
                    info.product
                );
                format!("{} {}", info.manufacturer, info.product)
            }
            None => {
                tracing::info!("SMBIOS baseboard info not available; using Generic Motherboard");
                "Generic Motherboard".to_string()
            }
        };

        // Create a generic Motherboard instance and add to list
        let mut m = Motherboard::new(board_name);

        // If pawn_manager present, assign to motherboard for sensor reads
        if let Some(pm) = &self.pawn_manager {
            m.set_pawn_manager(pm.clone());

            // Immediately call update so SuperIO / EC detection runs now (ensures modules like LpcIO are exercised)
            if let Err(e) = m.update() {
                tracing::warn!("Motherboard initial update failed: {}", e);
            }
        }

        self.motherboards.push(Box::new(m));
        tracing::info!(
            "Motherboard detection completed: {} found",
            self.motherboards.len()
        );
        Ok(())
    }

    pub fn motherboards(&self) -> &[Box<dyn crate::core::Hardware>] {
        &self.motherboards
    }

    /// 更新所有主板传感器。
    pub fn update_all(&mut self) -> crate::core::HardwareResult<()> {
        // Set pawn manager on per-motherboard implementation if needed
        if let Some(pm) = &self.pawn_manager {
            for m in &mut self.motherboards {
                if let Some(mb) = m.as_any_mut().downcast_mut::<Motherboard>() {
                    mb.set_pawn_manager(pm.clone());
                }
            }
        }

        for m in &mut self.motherboards {
            m.update()?;
        }

        Ok(())
    }
}
