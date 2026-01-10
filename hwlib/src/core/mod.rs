/// 硬件监控核心模块
///
/// 本模块定义了硬件监控系统的核心类型和接口，包括：
/// - 硬件类型和传感器类型的枚举
/// - 标识符、传感器值等数据结构
/// - 传感器、硬件、计算机等 trait
/// - 各种操作错误类型

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;
use thiserror::Error;

/// 硬件类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HardwareType {
    CPU,
    GPU,
    Motherboard,
    SuperIO,
    Memory,
    Storage,
    Network,
    PSU,
    Battery,
    Controller,
}

/// 传感器类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensorType {
    Voltage,      // V
    Current,      // A
    Power,        // W
    Clock,        // MHz
    Temperature,  // °C
    Load,         // %
    Frequency,    // Hz
    Fan,          // RPM
    Flow,         // L/h
    Control,      // %
    Level,        // %
    Factor,       // 1
    Data,         // GB = 2^30 Bytes
    SmallData,    // MB = 2^20 Bytes
    Throughput,   // B/s
    TimeSpan,     // Seconds
    Timing,       // ns
    Energy,       // milliwatt-hour (mWh)
    Noise,        // dBA
    Conductivity, // µS/cm
    Humidity,     // %
}

/// 控制模式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlMode {
    Undefined,
    Software,
    Default,
}

/// 硬件标识符
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identifier {
    pub hardware_type: HardwareType,
    pub instance: u32,
    pub name: String,
}

impl Identifier {
    pub fn new(hardware_type: HardwareType, instance: u32, name: impl Into<String>) -> Self {
        Self {
            hardware_type,
            instance,
            name: name.into(),
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}#{}/{}",
            self.hardware_type, self.instance, self.name
        )
    }
}

/// 带单位和时间戳的传感器值
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorValue {
    pub value: f32,
    pub unit: String,
    pub timestamp: std::time::SystemTime,
}

impl SensorValue {
    pub fn new(value: f32, unit: impl Into<String>) -> Self {
        Self {
            value,
            unit: unit.into(),
            timestamp: std::time::SystemTime::now(),
        }
    }
}

/// 带时间窗口的传感器值集合
#[derive(Debug, Clone)]
pub struct SensorValues {
    values: Vec<SensorValue>,
    time_window: std::time::Duration,
}

impl SensorValues {
    pub fn new(time_window: std::time::Duration) -> Self {
        Self {
            values: Vec::new(),
            time_window,
        }
    }

    pub fn add_value(&mut self, value: SensorValue) {
        let now = std::time::SystemTime::now();
        self.values
            .retain(|v| now.duration_since(v.timestamp).unwrap_or_default() < self.time_window);
        self.values.push(value);
    }

    pub fn values(&self) -> &[SensorValue] {
        &self.values
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn time_window(&self) -> std::time::Duration {
        self.time_window
    }

    pub fn set_time_window(&mut self, time_window: std::time::Duration) {
        self.time_window = time_window;
        // Clean up old values
        let now = std::time::SystemTime::now();
        self.values
            .retain(|v| now.duration_since(v.timestamp).unwrap_or_default() < self.time_window);
    }
}

/// 传感器 trait，定义基本的传感器接口
pub trait Sensor {
    fn identifier(&self) -> &Identifier;
    fn sensor_type(&self) -> SensorType;
    fn name(&self) -> &str;
    fn index(&self) -> usize;
    fn is_default_hidden(&self) -> bool;
    fn min_value(&self) -> Option<f32>;
    fn max_value(&self) -> Option<f32>;
    fn value(&self) -> Option<f32>;
    fn values(&self) -> &[SensorValue];
    fn values_time_window(&self) -> std::time::Duration;
    fn set_values_time_window(&mut self, window: std::time::Duration);
    fn control(&self) -> Option<&dyn Control>;
    fn parameters(&self) -> &[Box<dyn Parameter>];
    fn reset_min(&mut self);
    fn reset_max(&mut self);
    fn clear_values(&mut self);
    fn update(&mut self) -> Result<(), SensorError>;

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 控制 trait，定义传感器控制接口
pub trait Control {
    fn identifier(&self) -> &Identifier;
    fn control_mode(&self) -> ControlMode;
    fn sensor(&self) -> &dyn Sensor;
    fn min_software_value(&self) -> f32;
    fn max_software_value(&self) -> f32;
    fn software_value(&self) -> f32;
    fn set_default(&mut self) -> Result<(), ControlError>;
    fn set_software(&mut self, value: f32) -> Result<(), ControlError>;
}

/// 参数 trait，定义传感器参数接口
pub trait Parameter {
    fn identifier(&self) -> &Identifier;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn default_value(&self) -> f32;
    fn value(&self) -> f32;
    fn set_value(&mut self, value: f32) -> Result<(), ParameterError>;
    fn is_default(&self) -> bool;
    fn set_is_default(&mut self, is_default: bool);
    fn sensor(&self) -> &dyn Sensor;
}

/// 硬件 trait，定义基本的硬件接口
pub trait Hardware {
    fn identifier(&self) -> &Identifier;
    fn hardware_type(&self) -> HardwareType;
    fn name(&self) -> &str;
    fn parent(&self) -> Option<&dyn Hardware>;
    fn sensors(&self) -> &[Box<dyn Sensor>];
    fn sub_hardware(&self) -> &[Box<dyn Hardware>];
    fn properties(&self) -> &std::collections::HashMap<String, String>;
    fn get_report(&self) -> String;
    fn update(&mut self) -> Result<(), HardwareError>;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// 计算机 trait，定义主接口
pub trait Computer {
    fn hardware(&self) -> &[Box<dyn Hardware>];
    fn update(&mut self) -> Result<(), ComputerError>;
    fn close(&mut self);
}

/// 传感器事件处理器类型
pub type SensorEventHandler = Box<dyn Fn(&dyn Sensor) + Send + Sync>;

/// 具有事件处理能力的硬件
pub trait HardwareEvents {
    fn on_sensor_added(&mut self, handler: SensorEventHandler);
    fn on_sensor_removed(&mut self, handler: SensorEventHandler);
    fn trigger_sensor_added(&self, sensor: &dyn Sensor);
    fn trigger_sensor_removed(&self, sensor: &dyn Sensor);
}

/// 传感器操作错误
#[derive(Error, Debug)]
pub enum SensorError {
    #[error("Hardware access denied: {0}")]
    AccessDenied(String),
    #[error("Sensor read failed: {0}")]
    ReadFailed(String),
    #[error("Invalid sensor data: {0}")]
    InvalidData(String),
    #[error("Sensor timeout: {0}")]
    Timeout(String),
}

/// 硬件操作错误
#[derive(Error, Debug)]
pub enum HardwareError {
    #[error("Driver initialization failed: {0}")]
    DriverInitFailed(String),
    #[error("Hardware detection failed: {0}")]
    DetectionFailed(String),
    #[error("Sensor error: {0}")]
    SensorError(#[from] SensorError),
    #[error("Hardware communication error: {0}")]
    CommunicationError(String),
}

/// 计算机操作错误
#[derive(Error, Debug)]
pub enum ComputerError {
    #[error("Hardware error: {0}")]
    HardwareError(#[from] HardwareError),
    #[error("System not supported: {0}")]
    SystemNotSupported(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// 控制操作错误
#[derive(Error, Debug)]
pub enum ControlError {
    #[error("Control not supported: {0}")]
    NotSupported(String),
    #[error("Invalid control value: {0}")]
    InvalidValue(String),
    #[error("Control access denied: {0}")]
    AccessDenied(String),
}

/// 参数操作错误
#[derive(Error, Debug)]
pub enum ParameterError {
    #[error("Parameter not supported: {0}")]
    NotSupported(String),
    #[error("Invalid parameter value: {0}")]
    InvalidValue(String),
    #[error("Parameter access denied: {0}")]
    AccessDenied(String),
}

/// 便捷的结果类型
pub type SensorResult<T> = Result<T, SensorError>;
pub type HardwareResult<T> = Result<T, HardwareError>;
pub type ComputerResult<T> = Result<T, ComputerError>;
pub type ControlResult<T> = Result<T, ControlError>;
pub type ParameterResult<T> = Result<T, ParameterError>;
