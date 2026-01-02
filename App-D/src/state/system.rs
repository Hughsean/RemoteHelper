//! 系统监控状态管理
//!
//! 符合 Dioxus 0.7 官方推荐的状态管理模式

use dioxus::prelude::*;
use std::collections::VecDeque;

/// 系统监控数据（非响应式）
#[derive(Clone, PartialEq, Debug)]
pub struct SystemData {
    /// 当前系统状态
    pub current_status: Option<common::SystemInfo>,
    /// 历史数据（用于趋势图）
    pub history: VecDeque<(u64, common::SystemInfo)>,
    /// 刷新间隔（毫秒）
    pub refresh_interval: u64,
}

impl Default for SystemData {
    fn default() -> Self {
        Self {
            current_status: None,
            history: VecDeque::with_capacity(30 * 60), // 30分钟数据
            refresh_interval: 1000,                    // 默认1秒
        }
    }
}

impl SystemData {
    /// 最大时间窗口（毫秒）- 30分钟
    const MAX_TIME_WINDOW_MS: u64 = 30 * 60 * 1000;

    /// 更新系统状态
    pub fn update_status(&mut self, status: common::SystemInfo) {
        let timestamp = status.timestamp;

        // 更新当前状态
        self.current_status = Some(status.clone());

        // 更新历史数据
        self.history.push_back((timestamp, status));

        // 清理过期数据
        while self.history.len() > 1 {
            if let (Some(oldest), Some(newest)) = (self.history.front(), self.history.back()) {
                if newest.0 - oldest.0 > Self::MAX_TIME_WINDOW_MS {
                    self.history.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// 设置刷新间隔
    pub fn set_refresh_interval(&mut self, interval_ms: u64) {
        self.refresh_interval = interval_ms;
    }

    /// 清空历史数据
    pub fn clear_history(&mut self) {
        self.history = VecDeque::with_capacity(30 * 60);
    }

    /// 获取最近N分钟的数据
    pub fn get_recent_data(&self, minutes: u64) -> Vec<(u64, common::SystemInfo)> {
        if self.history.is_empty() {
            return Vec::new();
        }

        let window_ms = minutes * 60 * 1000;
        if let Some(newest) = self.history.back() {
            let cutoff = newest.0.saturating_sub(window_ms);
            self.history
                .iter()
                .filter(|(ts, _)| *ts >= cutoff)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// 系统状态的响应式包装（符合官方推荐）
pub type SystemState = Signal<SystemData>;

/// Hook: 获取系统监控状态
pub fn use_system_state() -> SystemState {
    use_context::<SystemState>()
}
