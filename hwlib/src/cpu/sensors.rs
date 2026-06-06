use crate::core::{Identifier, Sensor, SensorType, SensorValue};
use std::time::Duration;

/// CPU 的温度传感器
#[derive(Clone)]
pub struct TemperatureSensor {
    identifier: Identifier,
    name: String,
    value: Option<f64>,
    values: Vec<SensorValue>,
    time_window: Duration,
}

impl TemperatureSensor {
    pub fn new(identifier: Identifier, name: String) -> Self {
        Self {
            identifier,
            name,
            value: None,
            values: Vec::new(),
            time_window: Duration::from_secs(60),
        }
    }

    pub fn update_value(&mut self, value: f64) {
        self.value = Some(value);
        let sensor_value = SensorValue::new(value as f32, "°C");
        self.values.push(sensor_value);

        // Evict by count cap
        while self.values.len() > 100 {
            self.values.remove(0);
        }

        // Evict by time window
        let now = std::time::SystemTime::now();
        while let Some(oldest) = self.values.first() {
            match now.duration_since(oldest.timestamp) {
                Ok(age) if age > self.time_window => {
                    self.values.remove(0);
                }
                _ => break,
            }
        }
    }
}

impl Sensor for TemperatureSensor {
    fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::Temperature
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn value(&self) -> Option<f32> {
        self.value.map(|v| v as f32)
    }

    fn values(&self) -> &[SensorValue] {
        &self.values
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// CPU 的功率传感器
#[derive(Clone)]
pub struct PowerSensor {
    identifier: Identifier,
    name: String,
    value: Option<f64>,
    values: Vec<SensorValue>,
    time_window: Duration,
}

impl PowerSensor {
    pub fn new(identifier: Identifier, name: String) -> Self {
        Self {
            identifier,
            name,
            value: None,
            values: Vec::new(),
            time_window: Duration::from_secs(60),
        }
    }

    pub fn update_value(&mut self, value: f64) {
        self.value = Some(value);
        let sensor_value = SensorValue::new(value as f32, "W");
        self.values.push(sensor_value);

        // Evict by count cap
        while self.values.len() > 100 {
            self.values.remove(0);
        }

        // Evict by time window
        let now = std::time::SystemTime::now();
        while let Some(oldest) = self.values.first() {
            match now.duration_since(oldest.timestamp) {
                Ok(age) if age > self.time_window => {
                    self.values.remove(0);
                }
                _ => break,
            }
        }
    }
}

impl Sensor for PowerSensor {
    fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::Power
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn value(&self) -> Option<f32> {
        self.value.map(|v| v as f32)
    }

    fn values(&self) -> &[SensorValue] {
        &self.values
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
