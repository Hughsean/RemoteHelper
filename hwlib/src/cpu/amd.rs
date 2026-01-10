use crate::core::{Hardware, HardwareResult, HardwareType, Identifier, Sensor, SensorResult};
use crate::driver::{get_driver, pawn};
use crate::cpu::detection::CpuInfo;
use crate::cpu::sensors::{TemperatureSensor, VoltageSensor, PowerSensor};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::driver::PawnModuleManager;

use tracing;
pub struct AmdCpu {
    info: CpuInfo,
    identifier: Identifier,
    sensors: Vec<Box<dyn Sensor>>,
    properties: HashMap<String, String>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,

    last_energy_sample_time: Option<std::time::Instant>,
    last_pkg_energy: Option<u32>,
    energy_unit_joule: Option<f64>,
}

impl AmdCpu {
    pub fn new(info: CpuInfo) -> HardwareResult<Self> {
        let identifier = Identifier::new(HardwareType::CPU, 0, &info.name);

        let tctl_id = Identifier::new(HardwareType::CPU, 0, "Core (Tctl)");
        let tdie_id = Identifier::new(HardwareType::CPU, 0, "Core (Tdie)");
        let voltage_id = Identifier::new(HardwareType::CPU, 0, "CPU Core Voltage");
        let power_id = Identifier::new(HardwareType::CPU, 0, "CPU Package Power");

        let tctl_sensor = TemperatureSensor::new(tctl_id, "Core (Tctl)".to_string());
        let tdie_sensor = TemperatureSensor::new(tdie_id, "Core (Tdie)".to_string());
        let voltage_sensor = VoltageSensor::new(voltage_id, "CPU Core".to_string());
        let power_sensor = PowerSensor::new(power_id, "CPU Package".to_string());

        let sensors: Vec<Box<dyn Sensor>> = vec![
            Box::new(tctl_sensor) as Box<dyn Sensor>,
            Box::new(tdie_sensor) as Box<dyn Sensor>,
            Box::new(voltage_sensor) as Box<dyn Sensor>,
            Box::new(power_sensor) as Box<dyn Sensor>,
        ];

        let mut properties = HashMap::new();
        properties.insert("Manufacturer".to_string(), info.manufacturer.clone());
        properties.insert("Family".to_string(), format!("0x{:X}", info.family));
        properties.insert("Model".to_string(), format!("0x{:X}", info.model));
        properties.insert("Stepping".to_string(), format!("0x{:X}", info.stepping));
        properties.insert("Cores".to_string(), info.cores.to_string());
        properties.insert("Threads".to_string(), info.threads.to_string());

        Ok(Self {
            info,
            identifier,
            sensors,
            properties,
            pawn_manager: None,
            last_energy_sample_time: None,
            last_pkg_energy: None,
            energy_unit_joule: None,
        })
    }

    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        self.pawn_manager = Some(pm);
    }

    /// Read CPU temperature from SMU
    fn read_temperature(&mut self, pawn_manager: Option<&mut crate::driver::PawnModuleManager>) -> HardwareResult<f64> {
        if let Some(pm) = pawn_manager {
            // TODO: Read Tctl temperature from SMU register 0x00059800
            // For now, return dummy value
            Ok(45.0)
        } else {
            Ok(0.0) // No driver available
        }
    }

    /// Read CPU voltage
    fn read_voltage(&mut self, pawn_manager: Option<&mut crate::driver::PawnModuleManager>) -> HardwareResult<f64> {
        if let Some(pm) = pawn_manager {
            // TODO: Read voltage from MSR 0x198
            Ok(1.2)
        } else {
            Ok(0.0)
        }
    }

    /// Read CPU power consumption
    fn read_power(&mut self, pawn_manager: Option<&mut crate::driver::PawnModuleManager>) -> HardwareResult<f64> {
        if let Some(pm) = pawn_manager {
            // TODO: Read PPT from SMU register 0x00059808
            Ok(65.0)
        } else {
            Ok(0.0)
        }
    }
}

impl Hardware for AmdCpu {
    fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    fn hardware_type(&self) -> HardwareType {
        HardwareType::CPU
    }

    fn name(&self) -> &str {
        &self.info.name
    }

    fn parent(&self) -> Option<&dyn Hardware> {
        None
    }

    fn sensors(&self) -> &[Box<dyn Sensor>] {
        &self.sensors
    }

    fn sub_hardware(&self) -> &[Box<dyn Hardware>] {
        &[]
    }

    fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    fn get_report(&self) -> String {
        format!("AMD CPU: {} ({} cores, {} threads)",
                self.info.name, self.info.cores, self.info.threads)
    }

    fn update(&mut self) -> HardwareResult<()> {
        tracing::info!("Update called, family = {}", self.info.family);
        if let Some(pm) = &self.pawn_manager {
            let mut pm = pm.lock().unwrap();

            // Temperature: for Family 0x19 (Zen 3/4), use MSR 0x590; else SMN 0x00059800
            if self.info.family == 0x19 {
                // MSR 0x590 for Tdie on Zen 3/4
                if let Ok(msr) = pm.read_msr_amd(0x590) {
                    let data = (msr & 0xFFFF_FFFF) as u32;
                    let temp_raw = data & 0xfff;
                    let temp_c = (temp_raw as f64 * 0.0625) - 49.0;
                    tracing::info!("Read temperature MSR: raw={}, temp_c={}", temp_raw, temp_c);
                    if let Some(sensor) = self.sensors[0].as_any_mut().downcast_mut::<TemperatureSensor>() {
                        sensor.update_value(temp_c);
                    }
                } else {
                    tracing::warn!("Failed to read temperature from MSR 0x590");
                }
            } else {
                // SMN THM_TCON_CUR_TMP for Tctl/Tdie
                match pm.read_smn_amd(0x00059800) {
                    Ok(raw_temp) => {
                        let tctl_c = ((raw_temp >> 21) * 125) as f64 * 0.001;
                        let tdie_c = tctl_c - 65.0; // Offset for Ryzen 5000/7000
                        tracing::info!("Read temperature SMN: raw={}, Tctl={}, Tdie={}", raw_temp, tctl_c, tdie_c);
                        if let Some(sensor) = self.sensors[0].as_any_mut().downcast_mut::<TemperatureSensor>() {
                            sensor.update_value(tctl_c);
                        }
                        if let Some(sensor) = self.sensors[1].as_any_mut().downcast_mut::<TemperatureSensor>() {
                            sensor.update_value(tdie_c);
                        }
                    }
                    Err(e) => tracing::warn!("Failed to read temperature from SMN: {}", e),
                }
            }

            // Voltage: not implemented yet (SVI2 TFN telemetry via SMN varies by model)

            // Package power: derive from MSR energy counter (requires two samples)
            const MSR_PWR_UNIT: u32 = 0xC001_0299;
            const MSR_PKG_ENERGY_STAT: u32 = 0xC001_029B;

            if self.energy_unit_joule.is_none() {
                if let Ok(msr) = pm.read_msr_amd(MSR_PWR_UNIT) {
                    let eax = (msr & 0xFFFF_FFFF) as u32;
                    let esu = ((eax >> 8) & 0x1F) as i32;
                    // AMD/Intel convention: energy unit is in joules, as 1 / 2^ESU.
                    let energy_unit_joule = 0.5_f64.powi(esu);
                    self.energy_unit_joule = Some(energy_unit_joule);
                    tracing::debug!(
                        "Energy unit: esu={}, unit_uJ={}",
                        esu,
                        energy_unit_joule * 1_000_000.0
                    );
                }
            }

            if let (Some(energy_unit_joule), Ok(msr)) = (self.energy_unit_joule, pm.read_msr_amd(MSR_PKG_ENERGY_STAT)) {
                let eax = (msr & 0xFFFF_FFFF) as u32;
                let now = std::time::Instant::now();

                if let (Some(prev_t), Some(prev_e)) = (self.last_energy_sample_time, self.last_pkg_energy) {
                    let dt = now.duration_since(prev_t).as_secs_f64();
                    if dt > 0.0 {
                        let delta = eax.wrapping_sub(prev_e) as f64;
                        let joules = delta * energy_unit_joule;
                        let watts = joules / dt;

                        tracing::info!("Read package power: delta={}, dt_s={}, W={}", delta, dt, watts);
                        if let Some(sensor) = self.sensors[3].as_any_mut().downcast_mut::<PowerSensor>() {
                            sensor.update_value(watts);
                        }
                    }
                }

                self.last_energy_sample_time = Some(now);
                self.last_pkg_energy = Some(eax);
            }
        } else {
            tracing::warn!("No pawn_manager available for hardware reading");
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}











