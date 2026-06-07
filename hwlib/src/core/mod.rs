/// 硬件监控核心模块
///
/// 定义硬件监控系统的最小化核心类型和 trait，包括：
/// - 硬件类型和传感器类型的枚举
/// - 标识符、传感器值等数据结构
/// - 传感器、硬件 trait
/// - 错误类型
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt;
use thiserror::Error;

/// 硬件类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HardwareType {
    CPU,
    Motherboard,
}

/// 传感器类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensorType {
    /// 温度（°C）
    Temperature,
    /// 功率（W）
    Power,
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

/// 传感器 trait
pub trait Sensor: Send + Sync {
    fn identifier(&self) -> &Identifier;
    fn sensor_type(&self) -> SensorType;
    fn name(&self) -> &str;
    fn value(&self) -> Option<f32>;
    fn values(&self) -> &[SensorValue];
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 硬件 trait
pub trait Hardware: Send + Sync {
    fn identifier(&self) -> &Identifier;
    fn hardware_type(&self) -> HardwareType;
    fn name(&self) -> &str;
    fn sensors(&self) -> &[Box<dyn Sensor>];
    fn update(&mut self) -> Result<(), HardwareError>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
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

/// 便捷的结果类型
pub type HardwareResult<T> = Result<T, HardwareError>;
