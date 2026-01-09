use dioxus::prelude::*;
use crate::components::card::*;

const TREND_CHART_CSS: Asset = asset!("/assets/components/trend_chart/style.css");

/// 单个数据点
#[derive(Clone, PartialEq, Debug)]
pub struct DataPoint {
    pub value: f32,
    pub timestamp: u64,
}

/// 单个数据系列
#[derive(Clone, PartialEq)]
pub struct DataSeries {
    pub label: String,           // 系列名称
    pub data_points: Vec<DataPoint>,
    pub color: String,           // 线条颜色 (如 "#667eea")
    pub current_value: String,   // 当前值显示
    pub visible: bool,           // 是否可见
}

impl DataSeries {
    /// 添加新数据点
    pub fn add_point(&mut self, value: f32, timestamp: u64) {
        self.data_points.push(DataPoint { value, timestamp });
    }
}

/// 趋势图数据
#[derive(Clone, PartialEq)]
pub struct TrendChartData {
    pub title: String,
    pub unit: String,
    pub series: Vec<DataSeries>,  // 多个数据系列
    pub max_points: usize,        // 最多保留的数据点数量
}

impl TrendChartData {
    /// 添加新数据点到指定系列
    pub fn add_point_to_series(&mut self, series_index: usize, value: f32, timestamp: u64) {
        if let Some(series) = self.series.get_mut(series_index) {
            series.add_point(value, timestamp);

            // 保持最大点数限制
            if series.data_points.len() > self.max_points {
                series.data_points.remove(0);
            }
        }
    }

    /// 获取所有系列的数据范围
    fn get_range(&self) -> (f32, f32) {
        let mut all_values = Vec::new();

        for series in &self.series {
            for point in &series.data_points {
                all_values.push(point.value);
            }
        }

        if all_values.is_empty() {
            return (0.0, 100.0);
        }

        let min = all_values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = all_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        // 添加一些边距
        let range = max - min;
        let padding = range * 0.1;
        (
            (min - padding).max(0.0),
            (max + padding).min(100.0)
        )
    }
}

/// 趋势图组件 - 显示历史数据趋势（支持多系列）
#[component]
pub fn TrendChart(
    /// 图表数据（支持响应式更新）
    mut data: Signal<TrendChartData>,
) -> Element {
    let chart_data = data();

    // SVG 尺寸
    let width = 600.0;
    let height = 200.0;
    let padding = 5.0;  // 增加 padding 以容纳图例
    let chart_width = width - padding * 2.0;
    let chart_height = height - padding * 2.0;

    let (min_val, max_val) = chart_data.get_range();
    let range = max_val - min_val;

    // 为每个系列生成路径和填充（只处理可见的系列）
    let series_paths: Vec<(String, String, String)> = chart_data.series.iter()
        .filter(|series| series.visible)  // 只处理可见的系列
        .map(|series| {
            if series.data_points.len() >= 2 {
                // 生成线条路径
                let line_points: Vec<String> = series.data_points
                    .iter()
                    .enumerate()
                    .map(|(i, point)| {
                        let x = padding + (i as f32 / (series.data_points.len() - 1) as f32) * chart_width;
                        let normalized = if range > 0.0 {
                            (point.value - min_val) / range
                        } else {
                            0.5
                        };
                        let y = padding + chart_height - (normalized * chart_height);
                        format!("{},{}", x, y)
                    })
                    .collect();

                let line_path = if !line_points.is_empty() {
                    format!("M {}", line_points.join(" L "))
                } else {
                    String::new()
                };

                // 生成填充区域路径
                let mut fill_points: Vec<(f32, f32)> = series.data_points
                    .iter()
                    .enumerate()
                    .map(|(i, point)| {
                        let x = padding + (i as f32 / (series.data_points.len() - 1) as f32) * chart_width;
                        let normalized = if range > 0.0 {
                            (point.value - min_val) / range
                        } else {
                            0.5
                        };
                        let y = padding + chart_height - (normalized * chart_height);
                        (x, y)
                    })
                    .collect();

                let fill_path = if !fill_points.is_empty() {
                    let last_x = fill_points.last().unwrap().0;
                    let first_x = fill_points.first().unwrap().0;
                    fill_points.push((last_x, padding + chart_height));
                    fill_points.push((first_x, padding + chart_height));
                    let path_str: Vec<String> = fill_points.iter()
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

    // 计算总数据点数（取最大值）
    let total_points = chart_data.series.iter()
        .map(|s| s.data_points.len())
        .max()
        .unwrap_or(0);

    let has_data = chart_data.series.iter().any(|s| s.data_points.len() >= 2);

    rsx! {
        Card { class: "trend-chart-card",
            document::Link { rel: "stylesheet", href: TREND_CHART_CSS }

            CardHeader { class: "trend-chart-header",
                CardTitle { class: "trend-chart-title", "{chart_data.title}" }
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
                    div { class: "trend-chart-placeholder", "等待数据... ({total_points}/2)" }
                }
            }

            CardFooter { class: "trend-chart-footer", "{total_points} / {chart_data.max_points} 数据点" }
        }
    }
}
