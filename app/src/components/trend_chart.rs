use dioxus::prelude::*;
use crate::components::card::*;

const TREND_CHART_CSS: Asset = asset!("/assets/components/trend_chart/style.css");

/// 单个数据点
#[derive(Clone, PartialEq, Debug)]
pub struct DataPoint {
    pub value: f32,
    pub timestamp: u64,
}

/// 趋势图数据
#[derive(Clone, PartialEq)]
pub struct TrendChartData {
    pub title: String,
    pub current_value: String,
    pub unit: String,
    pub data_points: Vec<DataPoint>,
    pub max_points: usize,  // 最多保留的数据点数量
    pub color: String,      // 线条颜色 (如 "#667eea")
}

impl TrendChartData {
    /// 添加新数据点
    pub fn add_point(&mut self, value: f32, timestamp: u64) {
        self.data_points.push(DataPoint { value, timestamp });

        // 保持最大点数限制
        if self.data_points.len() > self.max_points {
            self.data_points.remove(0);
        }
    }

    /// 获取数据范围
    fn get_range(&self) -> (f32, f32) {
        if self.data_points.is_empty() {
            return (0.0, 100.0);
        }

        let values: Vec<f32> = self.data_points.iter().map(|p| p.value).collect();
        let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        // 添加一些边距
        let range = max - min;
        let padding = range * 0.1;
        (
            (min - padding).max(0.0),
            (max + padding).min(100.0)
        )
    }
}

/// 趋势图组件 - 显示历史数据趋势
#[component]
pub fn TrendChart(
    /// 图表数据（支持响应式更新）
    data: ReadSignal<TrendChartData>,
) -> Element {
    let chart_data = data();

    // SVG 尺寸
    let width = 600.0;
    let height = 200.0;
    let padding = 20.0;
    let chart_width = width - padding * 2.0;
    let chart_height = height - padding * 2.0;

    // 生成路径
    let path = if chart_data.data_points.len() >= 2 {
        let (min_val, max_val) = chart_data.get_range();
        let range = max_val - min_val;

        let points: Vec<String> = chart_data.data_points
            .iter()
            .enumerate()
            .map(|(i, point)| {
                let x = padding + (i as f32 / (chart_data.data_points.len() - 1) as f32) * chart_width;
                let normalized = if range > 0.0 {
                    (point.value - min_val) / range
                } else {
                    0.5
                };
                let y = padding + chart_height - (normalized * chart_height);
                format!("{},{}", x, y)
            })
            .collect();

        if points.is_empty() {
            String::new()
        } else {
            format!("M {}", points.join(" L "))
        }
    } else {
        String::new()
    };

    // 生成填充区域路径
    let fill_path = if chart_data.data_points.len() >= 2 {
        let (min_val, max_val) = chart_data.get_range();
        let range = max_val - min_val;

        let mut points: Vec<(f32, f32)> = chart_data.data_points
            .iter()
            .enumerate()
            .map(|(i, point)| {
                let x = padding + (i as f32 / (chart_data.data_points.len() - 1) as f32) * chart_width;
                let normalized = if range > 0.0 {
                    (point.value - min_val) / range
                } else {
                    0.5
                };
                let y = padding + chart_height - (normalized * chart_height);
                (x, y)
            })
            .collect();

        if !points.is_empty() {
            let last_x = points.last().unwrap().0;
            let first_x = points.first().unwrap().0;

            // 添加底部两个点形成闭合区域
            points.push((last_x, padding + chart_height));
            points.push((first_x, padding + chart_height));

            let path_str: Vec<String> = points.iter()
                .map(|(x, y)| format!("{},{}", x, y))
                .collect();

            format!("M {} Z", path_str.join(" L "))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    rsx! {
        Card { class: "trend-chart-card",
            document::Link { rel: "stylesheet", href: TREND_CHART_CSS }

            CardHeader { class: "trend-chart-header",
                CardTitle { class: "trend-chart-title", "{chart_data.title}" }
                div { class: "trend-chart-value",
                    span { class: "value-number", "{chart_data.current_value}" }
                    span { class: "value-unit", "{chart_data.unit}" }
                }
            }

            CardContent { class: "trend-chart-content",
                if chart_data.data_points.len() >= 2 {
                    svg {
                        class: "trend-chart-svg",
                        view_box: "0 0 {width} {height}",
                        preserve_aspect_ratio: "none",

                        // 填充区域
                        path {
                            d: "{fill_path}",
                            fill: "{chart_data.color}",
                            fill_opacity: "0.1",
                        }

                        // 趋势线
                        path {
                            d: "{path}",
                            stroke: "{chart_data.color}",
                            stroke_width: "2",
                            fill: "none",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                        }
                    }
                } else {
                    div { class: "trend-chart-placeholder",
                        "等待数据... ({chart_data.data_points.len()}/2)"
                    }
                }
            }

            CardFooter { class: "trend-chart-footer",
                "{chart_data.data_points.len()} / {chart_data.max_points} 数据点"
            }
        }
    }
}
