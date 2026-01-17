//! 高级传感器抽象层
//!
//! 提供一个统一的接口用于读取系统中 CPU 与主板传感器，供其他包调用。
//!
//! 设计要点：
//! - 使用 `CpuGroup` / `MotherboardGroup` 执行检测与更新
//! - 将传感器结果归一为 `SensorReading`，便于序列化与上层使用
//! - 由于当前主板传感器实现仍有问题（未完成寄存器读取等），所有来自主板的 sensor 返回值都**强制**为 `None`，以避免错误依赖

use crate::core::{Identifier, SensorType, SensorValue};
use crate::cpu::CpuGroup;
use crate::driver::PawnModuleManager;
#[cfg(feature = "motherboard_sensors")]
use crate::motherboard::MotherboardGroup;
use std::sync::{Arc, Mutex};

/// 单个传感器的统一读数表示
#[derive(Debug, Clone)]
pub struct SensorReading {
    pub identifier: Identifier,
    pub name: String,
    pub sensor_type: SensorType,
    /// 如果没有可用值则为 `None`。主板传感器在当前抽象中始终为 `None`。
    pub value: Option<SensorValue>,
}

/// 传感器管理器，封装 CPU 与主板的检测、更新与读取
pub struct SensorHub {
    cpu_group: CpuGroup,
    #[cfg(feature = "motherboard_sensors")]
    mb_group: MotherboardGroup,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl SensorHub {
    /// 创建新的 `SensorHub`。
    pub fn new() -> Self {
        Self {
            cpu_group: CpuGroup::new(),
            #[cfg(feature = "motherboard_sensors")]
            mb_group: MotherboardGroup::new(),
            pawn_manager: None,
        }
    }

    /// 将 PawnIO 管理器注入，用于硬件访问
    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        self.pawn_manager = Some(pm.clone());
        self.cpu_group.set_pawn_manager(pm.clone());
        #[cfg(feature = "motherboard_sensors")]
        self.mb_group.set_pawn_manager(pm);
    }

    /// 检测硬件（CPU / Motherboard）
    pub fn detect(&mut self) -> crate::core::HardwareResult<()> {
        self.cpu_group.detect_cpus()?;
        #[cfg(feature = "motherboard_sensors")]
        self.mb_group.detect_motherboards()?;
        Ok(())
    }

    /// 更新并读取所有传感器。返回 (cpu_sensors, motherboard_sensors)
    ///
    /// 注意：主板传感器的 `value` 被强制置为 `None`（当前主板实现不稳定）。
    #[cfg(feature = "motherboard_sensors")]
    pub fn read_all(
        &mut self,
    ) -> crate::core::HardwareResult<(Vec<SensorReading>, Vec<SensorReading>)> {
        // ensure pawn manager set on groups if present
        if let Some(pm) = &self.pawn_manager {
            self.cpu_group.set_pawn_manager(pm.clone());
            self.mb_group.set_pawn_manager(pm.clone());
        }

        // update sensors
        let _ = self.cpu_group.update_all();
        let _ = self.mb_group.update_all();

        // gather CPU sensors
        let mut cpu_readings: Vec<SensorReading> = Vec::new();
        for cpu in self.cpu_group.cpus() {
            for s in cpu.sensors() {
                let value = s.values().last().cloned();
                let reading = SensorReading {
                    identifier: s.identifier().clone(),
                    name: s.name().to_string(),
                    sensor_type: s.sensor_type(),
                    value,
                };
                cpu_readings.push(reading);
            }
        }

        // gather motherboard sensors - force values to None
        let mut mb_readings: Vec<SensorReading> = Vec::new();
        for mb in self.mb_group.motherboards() {
            for s in mb.sensors() {
                let reading = SensorReading {
                    identifier: s.identifier().clone(),
                    name: s.name().to_string(),
                    sensor_type: s.sensor_type(),
                    // Explicitly suppress values from motherboard sensors
                    value: None,
                };
                mb_readings.push(reading);
            }
        }

        Ok((cpu_readings, mb_readings))
    }

    #[cfg(not(feature = "motherboard_sensors"))]
    pub fn read_all(
        &mut self,
    ) -> crate::core::HardwareResult<(Vec<SensorReading>, Vec<SensorReading>)> {
        // ensure pawn manager set on CPU group if present
        if let Some(pm) = &self.pawn_manager {
            self.cpu_group.set_pawn_manager(pm.clone());
        }

        // update CPU sensors only
        let _ = self.cpu_group.update_all();

        // gather CPU sensors
        let mut cpu_readings: Vec<SensorReading> = Vec::new();
        for cpu in self.cpu_group.cpus() {
            for s in cpu.sensors() {
                let value = s.values().last().cloned();
                let reading = SensorReading {
                    identifier: s.identifier().clone(),
                    name: s.name().to_string(),
                    sensor_type: s.sensor_type(),
                    value,
                };
                cpu_readings.push(reading);
            }
        }

        // No motherboard support: return empty list
        let mb_readings: Vec<SensorReading> = Vec::new();
        Ok((cpu_readings, mb_readings))
    }
}

// Re-export常用类型
pub use SensorHub as Sensors;
pub use SensorReading as Sensor;
