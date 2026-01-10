// Module declarations
pub mod amd;
pub mod detection;
pub mod sensors;

// Re-export main types
pub use amd::AmdCpu;
pub use detection::{CpuInfo, CpuDetection};
pub use sensors::{TemperatureSensor, VoltageSensor, PowerSensor};

use std::sync::{Arc, Mutex};
use std::any::Any;
use crate::driver::PawnModuleManager;

use tracing;

/// CPU module placeholder - will be implemented as part of the roadmap
pub struct CpuGroup {
    cpus: Vec<Box<dyn crate::core::Hardware>>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl CpuGroup {
    pub fn new() -> Self {
        Self {
            cpus: Vec::new(),
            pawn_manager: None,
        }
    }

    pub fn detect_cpus(&mut self) -> crate::core::HardwareResult<()> {
        tracing::info!("Detecting CPUs...");
        self.cpus = crate::cpu::detection::CpuDetection::detect_cpus()?;
        tracing::info!("Detected {} CPU(s)", self.cpus.len());
        Ok(())
    }

    pub fn cpus(&self) -> &[Box<dyn crate::core::Hardware>] {
        &self.cpus
    }

    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        tracing::info!("Setting pawn_manager for CpuGroup");
        self.pawn_manager = Some(pm);
    }

    pub fn update_all(&mut self) -> crate::core::HardwareResult<()> {
        // Set pawn manager for AMD CPUs
        if let Some(pm) = &self.pawn_manager {
            println!("Setting pawn_manager for {} CPUs", self.cpus.len());
            tracing::info!("Setting pawn_manager for {} CPUs", self.cpus.len());
            for cpu in &mut self.cpus {
                if let Some(amd_cpu) = cpu.as_any_mut().downcast_mut::<AmdCpu>() {
                    println!("Setting pawn_manager for AmdCpu");
                    tracing::info!("Setting pawn_manager for AmdCpu");
                    amd_cpu.set_pawn_manager(pm.clone());
                } else {
                    println!("Failed to downcast CPU to AmdCpu");
                    tracing::warn!("Failed to downcast CPU to AmdCpu");
                }
            }
        } else {
            println!("No pawn_manager in CpuGroup");
        }

        for cpu in &mut self.cpus {
            cpu.update()?;
        }
        Ok(())
    }
}











