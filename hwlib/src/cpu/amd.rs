use crate::core::{Hardware, HardwareResult, HardwareType, Identifier, Sensor};
use crate::cpu::detection::CpuInfo;
use crate::cpu::sensors::{PowerSensor, TemperatureSensor};
use crate::driver::PawnModuleManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing;
/// AMD CPU 硬件监控器。
///
/// 支持 AMD Zen 架构处理器（Family 17h, 19h）的监控。
///
/// ## 传感器
///
/// - **温度**: Tctl（控制温度）和 Tdie（芯片温度）
/// - **电压**: 核心电压（通过 SVI2 遥测）
/// - **功率**: 封装功耗
///
/// ## 实现说明
///
/// - 温度从 MSR 0x590（Family 19h）或 SMN 0x00059800 读取
/// - 功率从 MSR 能量计数器（0xC001_029B）计算得出
/// - 需要具有 MSR/SMN 访问权限的 PawnIO 驱动
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
    /// 创建新的 AMD CPU 监控器。
    ///
    /// # 参数
    ///
    /// * `info` - CPU 标识信息
    ///
    /// # 错误
    ///
    /// 如果初始化失败则返回 [`HardwareError`](crate::core::HardwareError)。
    pub fn new(info: CpuInfo) -> HardwareResult<Self> {
        let identifier = Identifier::new(HardwareType::CPU, 0, &info.name);

        // let tctl_id = Identifier::new(HardwareType::CPU, 0, "Core (Tctl)");
        let tdie_id = Identifier::new(HardwareType::CPU, 0, "Core (Tdie)");
        let power_id = Identifier::new(HardwareType::CPU, 0, "CPU Package Power");

        // let tctl_sensor = TemperatureSensor::new(tctl_id, "CPU Package (Tctl)".to_string());
        let tdie_sensor = TemperatureSensor::new(tdie_id, "CPU Package (socket)".to_string());
        let power_sensor = PowerSensor::new(power_id, "CPU Package Power".to_string());

        let sensors: Vec<Box<dyn Sensor>> = vec![
            // Box::new(tctl_sensor) as Box<dyn Sensor>,
            Box::new(tdie_sensor) as Box<dyn Sensor>,
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

    /// 设置 PawnIO 驱动管理器。
    ///
    /// 在调用 [`update`](#method.update) 之前必须调用此方法以启用传感器读取。
    ///
    /// # 参数
    ///
    /// * `pm` - 共享的 PawnIO 模块管理器引用
    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        self.pawn_manager = Some(pm);
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
        format!(
            "AMD CPU: {} ({} cores, {} threads)",
            self.info.name, self.info.cores, self.info.threads
        )
    }

    fn update(&mut self) -> HardwareResult<()> {
        tracing::debug!("Update called, family = {}", self.info.family);
        if let Some(pm) = &self.pawn_manager {
            let mut pm = pm.lock().unwrap();

            // Temperature: for Family 0x19 (Zen 3/4), use MSR 0x590; else SMN 0x00059800
            if self.info.family == 0x19 {
                // MSR 0x590 for Tdie on Zen 3/4
                if let Ok(msr) = pm.read_msr_amd(0x590) {
                    let data = (msr & 0xFFFF_FFFF) as u32;
                    let temp_raw = data & 0xfff;
                    // Use package temperature (raw * 1/16)
                    let package_c = temp_raw as f64 * 0.0625;
                    tracing::debug!(
                        "Read temperature MSR: raw={}, package_c={}",
                        temp_raw,
                        package_c
                    );
                    if let Some(sensor) = self.sensors[0]
                        .as_any_mut()
                        .downcast_mut::<TemperatureSensor>()
                    {
                        sensor.update_value(package_c);
                    }
                    if let Some(sensor) = self.sensors[1]
                        .as_any_mut()
                        .downcast_mut::<TemperatureSensor>()
                    {
                        sensor.update_value(package_c);
                    }
                } else {
                    tracing::warn!("Failed to read temperature from MSR 0x590");
                }
            } else {
                // SMN THM_TCON_CUR_TMP for Tctl/Tdie
                match pm.read_smn_amd(0x00059800) {
                    Ok(raw_temp) => {
                        // THM_TCON_CUR_TMP: CUR_TEMP [31:21]
                        // If bit 19 (0x80000) is set, an additional -49°C offset applies.
                        let temp_offset_flag = (raw_temp & 0x80000) != 0;

                        // temperature in millidegrees = ((raw >> 21) * 125)
                        let mut t = ((raw_temp >> 21) * 125) as f64 * 0.001;
                        if temp_offset_flag {
                            t -= 49.0;
                        }

                        // Apply known model-specific offsets (from LibreHardwareMonitor / k10temp)
                        let mut name_offset: f64 = 0.0;
                        let cpu_name = self.info.name.as_str();
                        if cpu_name.contains("1600X")
                            || cpu_name.contains("1700X")
                            || cpu_name.contains("1800X")
                        {
                            name_offset = -20.0;
                        } else if cpu_name.contains("Threadripper 19")
                            || cpu_name.contains("Threadripper 29")
                        {
                            name_offset = -27.0;
                        } else if cpu_name.contains("2700X") {
                            name_offset = -10.0;
                        }

                        tracing::debug!(
                            "Read temperature SMN: raw={}, t={}, flag={}, name_offset={}",
                            raw_temp,
                            t,
                            temp_offset_flag,
                            name_offset
                        );

                        if name_offset < 0.0 {
                            if let Some(sensor) = self.sensors[0]
                                .as_any_mut()
                                .downcast_mut::<TemperatureSensor>()
                            {
                                sensor.update_value(t);
                            }
                            if let Some(sensor) = self.sensors[1]
                                .as_any_mut()
                                .downcast_mut::<TemperatureSensor>()
                            {
                                sensor.update_value(t + name_offset);
                            }
                        } else {
                            if let Some(sensor) = self.sensors[0]
                                .as_any_mut()
                                .downcast_mut::<TemperatureSensor>()
                            {
                                sensor.update_value(t);
                            }
                            if let Some(sensor) = self.sensors[1]
                                .as_any_mut()
                                .downcast_mut::<TemperatureSensor>()
                            {
                                sensor.update_value(t);
                            }
                        }
                    }
                    Err(e) => tracing::warn!("Failed to read temperature from SMN: {}", e),
                }
            }

            // Voltage: not implemented yet (SVI2 TFN telemetry via SMN varies by model)

            // Package power: derive from MSR energy counter (requires two samples)
            const MSR_PWR_UNIT: u32 = 0xC001_0299;
            const MSR_PKG_ENERGY_STAT: u32 = 0xC001_029B;

            if self.energy_unit_joule.is_none()
                && let Ok(msr) = pm.read_msr_amd(MSR_PWR_UNIT)
            {
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

            if let (Some(energy_unit_joule), Ok(msr)) =
                (self.energy_unit_joule, pm.read_msr_amd(MSR_PKG_ENERGY_STAT))
            {
                let eax = (msr & 0xFFFF_FFFF) as u32;
                let now = std::time::Instant::now();

                if let (Some(prev_t), Some(prev_e)) =
                    (self.last_energy_sample_time, self.last_pkg_energy)
                {
                    let dt = now.duration_since(prev_t).as_secs_f64();
                    if dt > 0.0 {
                        let delta = eax.wrapping_sub(prev_e) as f64;
                        let joules = delta * energy_unit_joule;
                        let watts = joules / dt;

                        tracing::debug!(
                            "Read package power: delta={}, dt_s={}, W={}",
                            delta,
                            dt,
                            watts
                        );
                        // Find PowerSensor dynamically to avoid relying on fixed index
                        for s in &mut self.sensors {
                            if let Some(sensor) = s.as_any_mut().downcast_mut::<PowerSensor>() {
                                sensor.update_value(watts);
                                break;
                            }
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
