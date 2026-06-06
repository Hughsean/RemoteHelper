use crate::core::{Hardware, HardwareResult, HardwareType, Identifier, Sensor};
use crate::cpu::detection::CpuInfo;
use crate::cpu::sensors::{PowerSensor, TemperatureSensor};
use crate::driver::PawnModuleManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing;

/// 缓存的温度传感器访问方式。
///
/// 首次探测后缓存，后续 `update()` 直接使用已确认的方法，
/// 避免重复的失败 MSR 读取和无用的警告日志。
#[derive(Debug, Clone, Copy, PartialEq)]
enum TemperatureSource {
    /// 尚未探测，首次 update() 时按优先级尝试
    Unprobed,
    /// MSR 0x590 可用（Zen 3/4 桌面端首选，速度最快）
    Msr0x590,
    /// SMN 0x00059800 可用（所有 Zen 架构通用，兼容性最广）
    Smn0x59800,
    /// 所有已知方法均失败，此 CPU 的温度传感器不可用
    Unavailable,
}

/// AMD CPU 硬件监控器。
///
/// 支持 AMD Zen 架构处理器（Family 17h, 19h）的监控。
///
/// ## 传感器
///
/// - **温度**: Tctl / Tdie（芯片温度）
/// - **功率**: 封装功耗
///
/// ## 温度读取策略（渐进式回退）
///
/// ```
/// Family 0x19 (Zen 3/4/5)
///   ├── MSR 0x590  ──失败──→  SMN 0x00059800  ──失败──→  Unavailable
/// Family 0x17 (Zen 1/2)
///   └── SMN 0x00059800  ──失败──→  Unavailable
/// Family 0x15/0x16 (Bulldozer/Jaguar)
///   └── 未来: SB-TSI via SMBus → Unavailable
/// ```
///
/// 首次成功后会缓存访问方式，后续更新直接使用缓存路径。
/// 如果已缓存的路径在后续更新中失败，会重新探测。
///
/// ## 功耗读取
///
/// 所有 Zen 架构统一使用 MSR 能量计数器：
/// - `MSR_PWR_UNIT` (0xC001_0299) 获取能量单位
/// - `MSR_PKG_ENERGY_STAT` (0xC001_029B) 获取累计能量
/// - 功耗 = Δ能量 / Δ时间（需要两次采样）
///
/// ## 依赖
///
/// - 需要具有 MSR/SMN 访问权限的 PawnIO 驱动
/// - 需要管理员权限
pub struct AmdCpu {
    info: CpuInfo,
    identifier: Identifier,
    sensors: Vec<Box<dyn Sensor>>,
    properties: HashMap<String, String>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,

    // 温度访问方式缓存
    temperature_source: TemperatureSource,

    // 功耗追踪
    last_energy_sample_time: Option<std::time::Instant>,
    last_pkg_energy: Option<u32>,
    energy_unit_joule: Option<f64>,
    /// 功耗 MSR 是否已确认可用（首次成功读取后置 true）
    power_probed: bool,
}

impl AmdCpu {
    /// 创建新的 AMD CPU 监控器。
    pub fn new(info: CpuInfo) -> HardwareResult<Self> {
        let identifier = Identifier::new(HardwareType::CPU, 0, &info.name);

        let tdie_id = Identifier::new(HardwareType::CPU, 0, "Core (Tdie)");
        let power_id = Identifier::new(HardwareType::CPU, 0, "CPU Package Power");

        let tdie_sensor = TemperatureSensor::new(tdie_id, "CPU Package (socket)".to_string());
        let power_sensor = PowerSensor::new(power_id, "CPU Package Power".to_string());

        let sensors: Vec<Box<dyn Sensor>> = vec![
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
            temperature_source: TemperatureSource::Unprobed,
            last_energy_sample_time: None,
            last_pkg_energy: None,
            energy_unit_joule: None,
            power_probed: false,
        })
    }

    /// 设置 PawnIO 驱动管理器。
    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        self.pawn_manager = Some(pm);
    }
}

impl AmdCpu {
    // ── 温度读取 ──────────────────────────────────────────────

    /// 尝试通过 MSR 0x590 读取温度（Zen 3/4 桌面端首选）
    fn try_read_temp_msr(pm: &mut PawnModuleManager) -> Option<f64> {
        let msr = pm.read_msr_amd(0x590).ok()?;
        let data = (msr & 0xFFFF_FFFF) as u32;
        let temp_raw = data & 0xfff;
        let package_c = temp_raw as f64 * 0.0625; // raw * 1/16
        tracing::debug!(
            "Temperature via MSR 0x590: raw=0x{:03X}, value={:.1}°C",
            temp_raw,
            package_c
        );
        Some(package_c)
    }

    /// 尝试通过 SMN 0x00059800 读取温度（所有 Zen 架构通用）
    fn try_read_temp_smn(pm: &mut PawnModuleManager, cpu_name: &str) -> Option<f64> {
        let raw_temp = pm.read_smn_amd(0x00059800).ok()?;

        // THM_TCON_CUR_TMP: CUR_TEMP [31:21]
        // Bit 19 (0x80000) set → additional -49°C offset
        let temp_offset_flag = (raw_temp & 0x80000) != 0;

        // temperature in millidegrees = ((raw >> 21) * 125)
        let mut t = ((raw_temp >> 21) * 125) as f64 * 0.001;
        if temp_offset_flag {
            t -= 49.0;
        }

        // Model-specific offsets (from LibreHardwareMonitor / k10temp)
        let name_offset: f64 = if cpu_name.contains("1600X")
            || cpu_name.contains("1700X")
            || cpu_name.contains("1800X")
        {
            -20.0
        } else if cpu_name.contains("Threadripper 19")
            || cpu_name.contains("Threadripper 29")
        {
            -27.0
        } else if cpu_name.contains("2700X") {
            -10.0
        } else {
            0.0
        };

        let final_temp = t + name_offset;
        tracing::debug!(
            "Temperature via SMN 0x59800: raw=0x{:08X}, t={:.1}, flag={}, offset={:.1}, final={:.1}°C",
            raw_temp, t, temp_offset_flag, name_offset, final_temp
        );
        Some(final_temp)
    }

    /// 统一的温度读取入口：根据缓存的 `temperature_source` 选择路径，
    /// 必要时回退到备选方案。
    fn read_temperature(&mut self, pm: &mut PawnModuleManager) {
        let source = self.temperature_source;

        match source {
            TemperatureSource::Unavailable => {
                // 已知不可用，跳过
                return;
            }
            TemperatureSource::Msr0x590 => {
                // 缓存了 MSR 路径，直接尝试
                if let Some(t) = Self::try_read_temp_msr(pm) {
                    self.update_temp_sensor(t);
                    return;
                }
                // MSR 突然失败了 → 回退到 SMN
                tracing::debug!("Cached MSR 0x590 failed, falling back to SMN");
                if let Some(t) = Self::try_read_temp_smn(pm, &self.info.name) {
                    self.temperature_source = TemperatureSource::Smn0x59800;
                    self.update_temp_sensor(t);
                    return;
                }
                self.temperature_source = TemperatureSource::Unavailable;
                tracing::warn!("All temperature access methods failed for {}", self.info.name);
            }
            TemperatureSource::Smn0x59800 => {
                // 缓存了 SMN 路径，直接尝试
                if let Some(t) = Self::try_read_temp_smn(pm, &self.info.name) {
                    self.update_temp_sensor(t);
                    return;
                }
                // SMN 失败了 → 重试 MSR（可能驱动重新初始化后路径变了）
                tracing::debug!("Cached SMN failed, retrying MSR 0x590");
                if let Some(t) = Self::try_read_temp_msr(pm) {
                    self.temperature_source = TemperatureSource::Msr0x590;
                    self.update_temp_sensor(t);
                    return;
                }
                self.temperature_source = TemperatureSource::Unavailable;
                tracing::warn!("All temperature access methods failed for {}", self.info.name);
            }
            TemperatureSource::Unprobed => {
                // 首次探测：按优先级尝试
                self.probe_temperature(pm);
            }
        }
    }

    /// 首次温度探测：按 family 决定优先级
    fn probe_temperature(&mut self, pm: &mut PawnModuleManager) {
        let is_zen3plus = self.info.family >= 0x19;

        if is_zen3plus {
            // Zen 3/4/5: 优先 MSR（快），失败回退 SMN（兼容）
            if let Some(t) = Self::try_read_temp_msr(pm) {
                tracing::info!(
                    "Temperature probe: MSR 0x590 available for {}",
                    self.info.name
                );
                self.temperature_source = TemperatureSource::Msr0x590;
                self.update_temp_sensor(t);
                return;
            }
            tracing::debug!("MSR 0x590 not available, falling back to SMN");
        }

        // Zen 1/2 或 Zen 3+ 的 MSR 回退
        if let Some(t) = Self::try_read_temp_smn(pm, &self.info.name) {
            tracing::info!(
                "Temperature probe: SMN 0x59800 available for {}",
                self.info.name
            );
            self.temperature_source = TemperatureSource::Smn0x59800;
            self.update_temp_sensor(t);
            return;
        }

        // 全部失败
        tracing::warn!(
            "Temperature probe: no method available for {} (family 0x{:X})",
            self.info.name,
            self.info.family
        );
        self.temperature_source = TemperatureSource::Unavailable;
    }

    /// 将温度值写入 TemperatureSensor
    fn update_temp_sensor(&mut self, value: f64) {
        for s in &mut self.sensors {
            if let Some(sensor) = s.as_any_mut().downcast_mut::<TemperatureSensor>() {
                sensor.update_value(value);
                return;
            }
        }
    }

    // ── 功耗读取 ──────────────────────────────────────────────

    /// 读取封装功耗（MSR 能量计数器，所有 Zen 通用）
    fn read_power(&mut self, pm: &mut PawnModuleManager) {
        const MSR_PWR_UNIT: u32 = 0xC001_0299;
        const MSR_PKG_ENERGY_STAT: u32 = 0xC001_029B;

        // 首次读取：获取能量单位
        if self.energy_unit_joule.is_none() {
            if let Ok(msr) = pm.read_msr_amd(MSR_PWR_UNIT) {
                let eax = (msr & 0xFFFF_FFFF) as u32;
                let esu = ((eax >> 8) & 0x1F) as i32;
                let energy_unit_joule = 0.5_f64.powi(esu);
                self.energy_unit_joule = Some(energy_unit_joule);
                tracing::debug!(
                    "Power probe: PWR_UNIT esu={}, unit_uJ={:.2}",
                    esu,
                    energy_unit_joule * 1_000_000.0
                );
            } else {
                if !self.power_probed {
                    tracing::warn!("Power probe: failed to read MSR_PWR_UNIT");
                }
            }
        }

        // 读取能量计数器
        if let (Some(energy_unit_joule), Ok(msr)) =
            (self.energy_unit_joule, pm.read_msr_amd(MSR_PKG_ENERGY_STAT))
        {
            if !self.power_probed {
                tracing::info!("Power probe: MSR_PKG_ENERGY_STAT available");
                self.power_probed = true;
            }

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
                        "Power: delta_energy={:.0}, dt_s={:.3}, W={:.2}",
                        delta,
                        dt,
                        watts
                    );
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
        } else if !self.power_probed {
            tracing::warn!("Power probe: failed to read MSR_PKG_ENERGY_STAT");
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
        format!(
            "AMD CPU: {} ({} cores, {} threads)",
            self.info.name, self.info.cores, self.info.threads
        )
    }

    fn update(&mut self) -> HardwareResult<()> {
        tracing::debug!("Update called, family = {}", self.info.family);
        let pm_arc = self.pawn_manager.clone();
        if let Some(pm_arc) = pm_arc {
            let mut pm = pm_arc.lock().unwrap();
            self.read_temperature(&mut pm);
            self.read_power(&mut pm);
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
