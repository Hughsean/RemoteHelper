use crate::core::{Control, Identifier, Parameter, Sensor, SensorType, SensorValue};
use std::collections::VecDeque;
use std::time::Duration;

/// CPU 的温度传感器
#[derive(Clone)]
pub struct TemperatureSensor {
    identifier: Identifier,
    name: String,
    value: Option<f64>,
    values: VecDeque<SensorValue>,
    time_window: Duration,
}

impl TemperatureSensor {
    pub fn new(identifier: Identifier, name: String) -> Self {
        Self {
            identifier,
            name,
            value: None,
            values: VecDeque::new(),
            time_window: Duration::from_secs(60),
        }
    }

    pub fn update_value(&mut self, value: f64) {
        self.value = Some(value);
        let sensor_value = SensorValue::new(value as f32, "°C");
        self.values.push_back(sensor_value);

        while self.values.len() > 100 {
            self.values.pop_front();
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

    fn index(&self) -> usize {
        0
    }

    fn is_default_hidden(&self) -> bool {
        false
    }

    fn min_value(&self) -> Option<f32> {
        self.values
            .iter()
            .map(|v| v.value)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn max_value(&self) -> Option<f32> {
        self.values
            .iter()
            .map(|v| v.value)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn value(&self) -> Option<f32> {
        self.value.map(|v| v as f32)
    }

    fn values(&self) -> &[SensorValue] {
        self.values.as_slices().0
    }

    fn values_time_window(&self) -> Duration {
        self.time_window
    }

    fn set_values_time_window(&mut self, window: Duration) {
        self.time_window = window;
    }

    fn control(&self) -> Option<&dyn Control> {
        None
    }

    fn parameters(&self) -> &[Box<dyn Parameter>] {
        &[]
    }

    fn reset_min(&mut self) {}
    fn reset_max(&mut self) {}
    fn clear_values(&mut self) {
        self.values.clear();
    }

    fn update(&mut self) -> Result<(), crate::core::SensorError> {
        Ok(())
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
    values: VecDeque<SensorValue>,
    time_window: Duration,
}

impl PowerSensor {
    pub fn new(identifier: Identifier, name: String) -> Self {
        Self {
            identifier,
            name,
            value: None,
            values: VecDeque::new(),
            time_window: Duration::from_secs(60),
        }
    }

    pub fn update_value(&mut self, value: f64) {
        self.value = Some(value);
        let sensor_value = SensorValue::new(value as f32, "W");
        self.values.push_back(sensor_value);

        while self.values.len() > 100 {
            self.values.pop_front();
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

    fn index(&self) -> usize {
        0
    }

    fn is_default_hidden(&self) -> bool {
        false
    }

    fn min_value(&self) -> Option<f32> {
        self.values
            .iter()
            .map(|v| v.value)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn max_value(&self) -> Option<f32> {
        self.values
            .iter()
            .map(|v| v.value)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn value(&self) -> Option<f32> {
        self.value.map(|v| v as f32)
    }

    fn values(&self) -> &[SensorValue] {
        self.values.as_slices().0
    }

    fn values_time_window(&self) -> Duration {
        self.time_window
    }

    fn set_values_time_window(&mut self, window: Duration) {
        self.time_window = window;
    }

    fn control(&self) -> Option<&dyn Control> {
        None
    }

    fn parameters(&self) -> &[Box<dyn Parameter>] {
        &[]
    }

    fn reset_min(&mut self) {}
    fn reset_max(&mut self) {}
    fn clear_values(&mut self) {
        self.values.clear();
    }

    fn update(&mut self) -> Result<(), crate::core::SensorError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
