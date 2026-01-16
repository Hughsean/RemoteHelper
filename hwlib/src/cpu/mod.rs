//! CPU 检测和监控模块。
//!
//! 提供检测和监控 CPU 指标的支持，包括：
//! - 温度（Tctl/Tdie）
//! - 电压
//! - 功耗
//!
//! 目前支持 AMD Zen 架构 CPU（Family 17h, 19h）。

// Module declarations
pub mod amd;
pub mod detection;
pub mod sensors;

// Re-export main types
pub use amd::AmdCpu;
pub use detection::{CpuDetection, CpuInfo};
pub use sensors::{PowerSensor, TemperatureSensor};

use crate::driver::PawnModuleManager;
use std::sync::{Arc, Mutex};

use tracing;

/// CPU 组管理器。
///
/// 管理检测到的 CPU 集合，并协调它们的更新。
/// 提供与 PawnIO 驱动的集成以实现低级硬件访问。
pub struct CpuGroup {
    cpus: Vec<Box<dyn crate::core::Hardware>>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl Default for CpuGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuGroup {
    /// 创建新的 CPU 组。
    ///
    /// 初始为空；调用 [`detect_cpus`](#method.detect_cpus) 来填充。
    pub fn new() -> Self {
        Self {
            cpus: Vec::new(),
            pawn_manager: None,
        }
    }

    /// 检测系统中的所有 CPU。
    ///
    /// 枚举系统 CPU 并初始化监控结构。
    ///
    /// # 错误
    ///
    /// 如果检测失败则返回 [`HardwareError`](crate::core::HardwareError)。
    pub fn detect_cpus(&mut self) -> crate::core::HardwareResult<()> {
        tracing::info!("Detecting CPUs...");
        self.cpus = crate::cpu::detection::CpuDetection::detect_cpus()?;
        tracing::info!("Detected {} CPU(s)", self.cpus.len());
        Ok(())
    }

    pub fn cpus(&self) -> &[Box<dyn crate::core::Hardware>] {
        &self.cpus
    }

    /// 设置 PawnIO 驱动管理器以实现低级硬件访问。
    ///
    /// 在调用 [`update_all`](#method.update_all) 之前必须调用此方法以启用传感器读取。
    ///
    /// # 参数
    ///
    /// * `pm` - 共享的 PawnIO 模块管理器引用
    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        tracing::info!("Setting pawn_manager for CpuGroup");
        self.pawn_manager = Some(pm);
    }

    /// 更新所有 CPU 传感器。
    ///
    /// 从硬件读取温度、电压和功率指标。
    /// 需要通过 [`set_pawn_manager`](#method.set_pawn_manager) 初始化 PawnIO 驱动。
    ///
    /// # 错误
    ///
    /// 如果传感器读取失败则返回 [`HardwareError`](crate::core::HardwareError)。
    pub fn update_all(&mut self) -> crate::core::HardwareResult<()> {
        // Set pawn manager for AMD CPUs
        if let Some(pm) = &self.pawn_manager {
            tracing::debug!("Setting pawn_manager for {} CPUs", self.cpus.len());
            for cpu in &mut self.cpus {
                if let Some(amd_cpu) = cpu.as_any_mut().downcast_mut::<AmdCpu>() {
                    tracing::debug!("Setting pawn_manager for AmdCpu");
                    amd_cpu.set_pawn_manager(pm.clone());
                } else {
                    tracing::warn!("Failed to downcast CPU to AmdCpu");
                }
            }
        } else {
            tracing::debug!("No pawn_manager available in CpuGroup");
        }

        for cpu in &mut self.cpus {
            cpu.update()?;
        }
        Ok(())
    }
}
