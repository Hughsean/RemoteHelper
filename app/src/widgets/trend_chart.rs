use crate::widgets::card::*;
use dioxus::prelude::*;
use dioxus_primitives::select::{Select, SelectList, SelectOption, SelectTrigger, SelectValue};
use std::collections::VecDeque;

const TREND_CHART_CSS: Asset = asset!("/assets/components/trend_chart/style.css");
const SELECT_CSS: Asset = asset!("/assets/components/select/style.css");

/// 时间窗口枚举
#[derive(Clone, PartialEq, Debug)]
pub enum TimeWindow {
    Minute1,
    Minute5,
    Minute30,
    Hour3,
}

impl TimeWindow {
    pub fn to_display(&self) -> &'static str {
        match self {
            TimeWindow::Minute1 => "1分钟",
            TimeWindow::Minute5 => "5分钟",
            TimeWindow::Minute30 => "30分钟",
            TimeWindow::Hour3 => "3小时",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "1分钟" => TimeWindow::Minute1,
            "5分钟" => TimeWindow::Minute5,
            "30分钟" => TimeWindow::Minute30,
            "3小时" => TimeWindow::Hour3,
            _ => TimeWindow::Minute5,
        }
    }
}

/// 获取当前时间戳（毫秒）
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// 单个数据点
#[derive(Clone, PartialEq, Debug)]
pub struct DataPoint {
    pub value: f32,
    pub timestamp: u64,
}

/// 单个数据系列
#[derive(Clone, PartialEq)]
pub struct DataSeries {
    pub label: String,                    // 系列名称
    pub data_points: VecDeque<DataPoint>, // 使用 VecDeque 高效删除旧数据
    pub color: String,                    // 线条颜色 (如 "#667eea")
    pub current_value: String,            // 当前值显示
    pub visible: bool,                    // 是否可见
}

impl DataSeries {
    /// 创建新的数据系列
    pub fn new(label: String, color: String) -> Self {
        Self {
            label,
            data_points: VecDeque::with_capacity(3 * 60 * 60), // 预分配 10800 个数据点容量（3小时，每秒1点）
            color,
            current_value: "0.0".to_string(),
            visible: true,
        }
    }

    /// 添加新数据点
    pub fn add_point(&mut self, value: f32, timestamp: u64) {
        self.data_points.push_back(DataPoint { value, timestamp });
    }

    /// 使用二分查找定位时间窗口的起始索引（数据点按时间戳有序）
    fn find_window_start(&self, cutoff_time: u64) -> usize {
        match self
            .data_points
            .binary_search_by_key(&cutoff_time, |p| p.timestamp)
        {
            Ok(idx) => idx,
            Err(idx) => idx,
        }
    }
}

/// 趋势图数据
#[derive(Clone, PartialEq)]
pub struct TrendChartData {
    pub title: String,
    pub unit: String,
    pub series: Vec<DataSeries>, // 多个数据系列
    pub time_window: TimeWindow, // 时间窗口
}

impl TrendChartData {
    /// 创建新的趋势图数据
    pub fn new(title: String, unit: String) -> Self {
        Self {
            title,
            unit,
            series: Vec::with_capacity(3), // 预分配 8 个系列容量
            time_window: TimeWindow::Minute5,
        }
    }

    /// 添加数据系列
    pub fn add_series(&mut self, series: DataSeries) {
        self.series.push(series);
    }

    /// 添加新数据点到指定系列
    pub fn add_point_to_series(&mut self, series_index: usize, value: f32, timestamp: u64) {
        if let Some(series) = self.series.get_mut(series_index) {
            series.add_point(value, timestamp);

            // 保留最近 3 小时 5 分钟的数据 - 使用 pop_front 高效删除旧数据
            let max_retention_millis = 3 * 3600 * 1000 + 5 * 60 * 1000;
            let cutoff_time = timestamp.saturating_sub(max_retention_millis);

            // 从前面删除过期数据（VecDeque 的 O(1) 操作）
            while let Some(front) = series.data_points.front() {
                if front.timestamp < cutoff_time {
                    series.data_points.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    /// 获取所有系列的数据范围（仅计算可见系列和时间窗口内的数据）
    fn get_range(&self, current_time: u64, window_millis: u64) -> (f32, f32) {
        let cutoff_time = current_time.saturating_sub(window_millis);
        let mut min_val = f32::INFINITY;
        let mut max_val = f32::NEG_INFINITY;
        let mut has_data = false;

        for series in &self.series {
            if !series.visible {
                continue;
            }

            // 使用二分查找定位起始点，避免遍历所有数据
            let start_idx = series.find_window_start(cutoff_time);

            // 只遍历时间窗口内的数据
            for point in series.data_points.iter().skip(start_idx) {
                if point.timestamp > current_time {
                    break; // 数据点有序，超出范围直接退出
                }
                min_val = min_val.min(point.value);
                max_val = max_val.max(point.value);
                has_data = true;
            }
        }

        if !has_data {
            return (0.0, 100.0);
        }

        // 添加一些边距
        let range = max_val - min_val;
        let padding = range * 0.1;
        ((min_val - padding).max(0.0), (max_val + padding).min(100.0))
    }
}

/// 趋势图组件 - 显示历史数据趋势（支持多系列）
#[component]
pub fn TrendChart(
    /// 图表数据（支持响应式更新）
    mut data: Signal<TrendChartData>,
) -> Element {
    let chart_data = data();

    // 获取当前时间和时间窗口（毫秒）
    let current_time = current_timestamp();
    let window_millis = match chart_data.time_window {
        TimeWindow::Minute1 => 60 * 1000,
        TimeWindow::Minute5 => 300 * 1000,
        TimeWindow::Minute30 => 1800 * 1000,
        TimeWindow::Hour3 => 10800 * 1000,
    };

    // 取可用数据的最早时间，数据不足窗口时用它作为起点，避免右侧留白
    let window_start = current_time.saturating_sub(window_millis);
    let (earliest_ts, latest_ts) = chart_data
        .series
        .iter()
        .filter(|s| s.visible)
        .flat_map(|s| s.data_points.iter().map(|p| p.timestamp))
        .fold(
            (Option::<u64>::None, Option::<u64>::None),
            |(min, max), ts| {
                let min_ts = match min {
                    Some(m) => Some(m.min(ts)),
                    None => Some(ts),
                };
                let max_ts = match max {
                    Some(m) => Some(m.max(ts)),
                    None => Some(ts),
                };
                (min_ts, max_ts)
            },
        );

    let time_start = earliest_ts
        .map(|ts| ts.max(window_start))
        .unwrap_or(window_start);
    // 数据稀疏时用最新数据作为 time_end，避免 current_time 过大导致右侧留白；但保证不小于 time_start
    let time_end = latest_ts.unwrap_or(current_time).max(time_start + 1);

    // SVG 尺寸
    let width = 600.0;
    let height = 200.0;
    let padding = 5.0; // 增加 padding 以容纳图例
    let chart_width = width - padding * 2.0;
    let chart_height = height - padding * 2.0;

    let (min_val, max_val) = chart_data.get_range(current_time, window_millis);
    let range = max_val - min_val;

    // 为每个系列生成路径和填充（只处理可见的系列和时间窗口内的数据）
    let series_paths: Vec<(String, String, String)> = chart_data
        .series
        .iter()
        .filter(|series| series.visible) // 只处理可见的系列
        .map(|series| {
            // 使用二分查找快速定位时间窗口，避免全量过滤
            let start_idx = series.find_window_start(time_start);
            let mut filtered_points: Vec<&DataPoint> = Vec::with_capacity(4096); // 预分配 4096 个引用容量
            filtered_points.extend(
                series
                    .data_points
                    .iter()
                    .skip(start_idx)
                    .take_while(|p| p.timestamp <= time_end),
            );

            if !filtered_points.is_empty() {
                // 生成线条路径 - 根据时间戳计算 X 坐标
                let mut line_points: Vec<String> = Vec::with_capacity(filtered_points.len()); // 预分配与数据点数量相同的容量
                line_points.extend(filtered_points.iter().map(|point| {
                    // X 坐标根据时间戳在时间窗口中的相对位置计算
                    let time_ratio = if time_end > time_start {
                        ((point.timestamp - time_start) as f32) / ((time_end - time_start) as f32)
                    } else {
                        0.0
                    };
                    let x = padding + time_ratio * chart_width;

                    let normalized = if range > 0.0 {
                        (point.value - min_val) / range
                    } else {
                        0.5
                    };
                    let y = padding + chart_height - (normalized * chart_height);
                    format!("{},{}", x, y)
                }));

                let line_path = if !line_points.is_empty() {
                    format!("M {}", line_points.join(" L "))
                } else {
                    String::new()
                };

                // 生成填充区域路径 - 根据时间戳计算 X 坐标
                let mut fill_points: Vec<(f32, f32)> =
                    Vec::with_capacity(filtered_points.len() + 2); // 预分配数据点数量+2的容量
                fill_points.extend(filtered_points.iter().map(|point| {
                    // X 坐标根据时间戳在时间窗口中的相对位置计算
                    let time_ratio = if time_end > time_start {
                        ((point.timestamp - time_start) as f32) / ((time_end - time_start) as f32)
                    } else {
                        0.0
                    };
                    let x = padding + time_ratio * chart_width;

                    let normalized = if range > 0.0 {
                        (point.value - min_val) / range
                    } else {
                        0.5
                    };
                    let y = padding + chart_height - (normalized * chart_height);
                    (x, y)
                }));

                let fill_path = if !fill_points.is_empty() {
                    let last_x = fill_points.last().unwrap().0;
                    let first_x = fill_points.first().unwrap().0;
                    fill_points.push((last_x, padding + chart_height));
                    fill_points.push((first_x, padding + chart_height));
                    let path_str: Vec<String> = fill_points
                        .iter()
                        .map(|(x, y)| format!("{},{}", x, y))
                        .collect();
                    format!("M {} Z", path_str.join(" L "))
                } else {
                    String::new()
                };

                (line_path, fill_path, series.color.clone())
            } else {
                (String::new(), String::new(), series.color.clone())
            }
        })
        .collect();

    // 计算时间窗口内总数据点数
    let total_points = chart_data
        .series
        .iter()
        .map(|s| {
            s.data_points
                .iter()
                .filter(|p| p.timestamp >= current_time.saturating_sub(window_millis))
                .count()
        })
        .sum::<usize>();

    let has_data = total_points >= 1;

    let time_window_str = match chart_data.time_window {
        TimeWindow::Minute1 => "1m",
        TimeWindow::Minute5 => "5m",
        TimeWindow::Minute30 => "30m",
        TimeWindow::Hour3 => "3h",
    };

    rsx! {
        Card { class: "trend-chart-card",
            document::Link { rel: "stylesheet", href: TREND_CHART_CSS }
            document::Link { rel: "stylesheet", href: SELECT_CSS }

            CardHeader { class: "trend-chart-header",
                div { class: "trend-chart-header-top",
                    CardTitle { class: "trend-chart-title", "{chart_data.title}" }
                    div { class: "time-window-selector",
                        label { class: "time-window-label", "时间窗口:" }
                        Select::<String> {
                            class: "select",
                            default_value: Some(data().time_window.to_display().to_string()),
                            on_value_change: move |value: Option<String>| {
                                if let Some(v) = value {
                                    data.write().time_window = TimeWindow::from_str(&v);
                                }
                            },
                            SelectTrigger { class: "select-trigger",
                                SelectValue { placeholder: chart_data.time_window.to_display().to_string() }
                                svg {
                                    class: "select-expand-icon",
                                    view_box: "0 0 24 24",
                                    xmlns: "http://www.w3.org/2000/svg",
                                    polyline { points: "6 9 12 15 18 9" }
                                }
                            }
                            SelectList { class: "select-list",
                                SelectOption::<String> {
                                    class: "select-option",
                                    index: use_signal(|| 0),
                                    value: "1分钟".to_string(),
                                    "1分钟"
                                }
                                SelectOption::<String> {
                                    class: "select-option",
                                    index: use_signal(|| 1),
                                    value: "5分钟".to_string(),
                                    "5分钟"
                                }
                                SelectOption::<String> {
                                    class: "select-option",
                                    index: use_signal(|| 2),
                                    value: "30分钟".to_string(),
                                    "30分钟"
                                }
                                SelectOption::<String> {
                                    class: "select-option",
                                    index: use_signal(|| 3),
                                    value: "3小时".to_string(),
                                    "3小时"
                                }
                            }
                        }
                    }
                }
                div { class: "trend-chart-legend",
                    for (index , series) in chart_data.series.iter().enumerate() {
                        div {
                            class: if series.visible { "legend-item" } else { "legend-item legend-item-hidden" },
                            onclick: move |_| {
                                let current_visible = data.read().series[index].visible;
                                data.write().series[index].visible = !current_visible;
                            },
                            span {
                                class: if series.visible { "legend-color" } else { "legend-color legend-color-hidden" },
                                style: "background-color: {series.color};",
                            }
                            span { class: "legend-label", "{series.label}" }
                            span { class: "legend-value", "{series.current_value}{chart_data.unit}" }
                        }
                    }
                }
            }

            CardContent { class: "trend-chart-content",
                if has_data {
                    svg {
                        class: "trend-chart-svg",
                        view_box: "0 0 {width} {height}",
                        preserve_aspect_ratio: "none",

                        // 渲染每个系列
                        for (line_path , fill_path , color) in &series_paths {
                            // 填充区域
                            if !fill_path.is_empty() {
                                path {
                                    d: "{fill_path}",
                                    fill: "{color}",
                                    fill_opacity: "0.1",
                                }
                            }

                            // 趋势线
                            if !line_path.is_empty() {
                                path {
                                    d: "{line_path}",
                                    stroke: "{color}",
                                    stroke_width: "2",
                                    fill: "none",
                                    stroke_linecap: "round",
                                    stroke_linejoin: "round",
                                }
                            }
                        }
                    }
                } else {
                    div { class: "trend-chart-placeholder",
                        "无数据显示 (总计: {total_points} 点, 时间窗口: {time_window_str})"
                        br {}
                        "可能原因: 数据不在当前时间窗口内，请尝试切换更长的时间窗口"
                    }
                }
            }

            CardFooter { class: "trend-chart-footer", "{total_points} 数据点 ({time_window_str})" }
        }
    }
}
