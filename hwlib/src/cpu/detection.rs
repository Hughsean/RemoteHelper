use crate::core::{Hardware, HardwareResult};
use crate::cpu::amd::AmdCpu;
use raw_cpuid::CpuId;

/// CPU information structure
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub name: String,
    pub manufacturer: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cores: u32,
    pub threads: u32,
}

/// CPU detection and enumeration
pub struct CpuDetection;

impl CpuDetection {
    /// Detect all CPUs in the system
    pub fn detect_cpus() -> HardwareResult<Vec<Box<dyn Hardware>>> {
        let cpu_info = Self::get_cpu_info()?;
        let mut cpus = Vec::new();

        if cpu_info.manufacturer == "Intel" {
            tracing::warn!(
                "Intel CPU detected: {}. Temperature/power monitoring is not supported on Intel (hwlib only supports AMD Zen).",
                cpu_info.name
            );
        }

        tracing::info!(
            "Detected CPU: {} ({}, family=0x{:X}, model=0x{:X}, stepping={}, {} cores, {} threads)",
            cpu_info.name,
            cpu_info.manufacturer,
            cpu_info.family,
            cpu_info.model,
            cpu_info.stepping,
            cpu_info.cores,
            cpu_info.threads,
        );

        let amd_cpu = AmdCpu::new(cpu_info)?;
        cpus.push(Box::new(amd_cpu) as Box<dyn Hardware>);

        Ok(cpus)
    }

    /// Get CPU information from CPUID
    pub fn get_cpu_info() -> HardwareResult<CpuInfo> {
        let cpuid = CpuId::new();

        // Vendor identification
        let vendor_str = cpuid
            .get_vendor_info()
            .map(|v| v.as_str().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let manufacturer = match vendor_str.as_str() {
            "AuthenticAMD" => "AMD".to_string(),
            "GenuineIntel" => "Intel".to_string(),
            other => other.to_string(),
        };

        // Family / Model / Stepping (already computed by raw-cpuid)
        let feature_info = cpuid.get_feature_info();
        let family = feature_info
            .as_ref()
            .map(|f| f.family_id() as u32)
            .unwrap_or(0x17);
        let model = feature_info
            .as_ref()
            .map(|f| f.model_id() as u32)
            .unwrap_or(0);
        let stepping = feature_info
            .as_ref()
            .map(|f| f.stepping_id() as u32)
            .unwrap_or(0);

        // Processor brand string (e.g. "AMD Ryzen 7 5800X 8-Core Processor")
        let name = cpuid
            .get_processor_brand_string()
            .map(|b| b.as_str().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{} CPU Family {:X}h", manufacturer, family));

        // Thread count from extended leaf 0x8000_0008 ECX[7:0] (NC + 1)
        let threads = cpuid
            .get_processor_capacity_feature_info()
            .map(|c| c.num_phys_threads() as u32)
            .unwrap_or(0);

        let threads = if threads > 0 {
            threads
        } else {
            // Fallback to OS-reported parallelism
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(8)
        };

        // Core count from topology leaf 0x8000_001E (AMD Zen)
        let cores = cpuid
            .get_processor_topology_info()
            .map(|t| {
                let tpc = t.threads_per_core() as u32;
                threads.checked_div(tpc).unwrap_or(threads)
            })
            .unwrap_or_else(|| {
                // Fallback: SMT=2 assumed for AMD, 1 for Intel
                if manufacturer == "AMD" {
                    threads.div_ceil(2)
                } else {
                    threads
                }
            });

        let cores = cores.clamp(1, threads);

        Ok(CpuInfo {
            name,
            manufacturer,
            family,
            model,
            stepping,
            cores,
            threads,
        })
    }
}
