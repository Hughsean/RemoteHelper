//! # HWLib - 硬件监控库
//!
//! 一个用于 Windows 系统低级硬件监控的 Rust 库。
//! 提供读取 CPU 传感器、GPU 信息和其他硬件指标的接口。
//!
//! ## 功能特性
//!
//! - CPU 温度、电压和功率监控（AMD CPU）
//! - 低级驱动接口（PawnIO 内核驱动）
//! - MSR（模型特定寄存器）访问
//! - AMD CPU 的 SMN（系统管理网络）访问
//! - 可扩展的硬件 trait 系统
//!
//! ## 架构
//!
//! - `core`: 硬件抽象的核心 trait 和类型
//! - `driver`: Windows 内核驱动接口（PawnIO）
//! - `cpu`: CPU 检测和监控
//! - `motherboard`: 主板传感器支持（计划中）
//!
//! ## 安全
//!
//! 此库需要管理员权限来加载内核驱动。
//! MSR/SMN 操作不当可能导致系统不稳定。

pub mod core;
pub mod cpu;
pub mod driver;
pub mod motherboard;

// High level sensors abstraction (CPU + Motherboard)
pub mod sensors;

// Re-export key types from sensors for convenience
pub use sensors::{Sensor, Sensors};
