use common::SystemInfo;
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum TimeWindow {
    OneMin,
    FiveMin,
    All,
}

impl TimeWindow {
    fn label(&self) -> &'static str {
        match self {
            TimeWindow::OneMin => "1分钟",
            TimeWindow::FiveMin => "5分钟",
            TimeWindow::All => "全部",
        }
    }
}

#[component]
pub fn TrendChart(history: Vec<(u64, SystemInfo)>) -> Element {
    let mut time_window = use_signal(|| TimeWindow::OneMin);

    if history.is_empty() {
        return rsx! {};
    }

    let now = js_sys::Date::now() as u64;
    let filtered_history: Vec<&SystemInfo> = history
        .iter()
        .filter(|(t, _)| match time_window() {
            TimeWindow::OneMin => *t > now.saturating_sub(60 * 1000),
            TimeWindow::FiveMin => *t > now.saturating_sub(5 * 60 * 1000),
            TimeWindow::All => true,
        })
        .map(|(_, s)| s)
        .collect();

    let width = 100.0;
    let height = 100.0;
    let margin_top = 5.0; // 顶部留白
    let margin_bottom = 5.0; // 底部留白
    let chart_height = height - margin_top - margin_bottom;

    // 辅助函数：自适应缩放数据并转换为平滑 SVG path 字符串
    let make_smooth_path = |extractor: fn(&SystemInfo) -> f32| -> String {
        if filtered_history.is_empty() {
            return String::new();
        }

        // 收集所有数据点
        let values: Vec<f32> = filtered_history
            .iter()
            .map(|info| extractor(info).clamp(0.0, 100.0))
            .collect();

        if values.is_empty() || values.len() < 2 {
            return String::new();
        }

        // 找到最小值和最大值
        let min_val = values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_val = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        // 计算动态范围，最小范围为10%以保证可见变化
        let range = (max_val - min_val).max(10.0);
        let center = (max_val + min_val) / 2.0;
        let scale_min = (center - range / 2.0).max(0.0);
        let scale_max = (center + range / 2.0).min(100.0);

        // 生成坐标点
        let points: Vec<(f32, f32)> = values
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = (i as f32 / (values.len().max(2) - 1) as f32) * width;
                let normalized = if scale_max > scale_min {
                    (val - scale_min) / (scale_max - scale_min)
                } else {
                    0.5
                };
                let y = margin_top + (1.0 - normalized) * chart_height;
                (x, y)
            })
            .collect();

        if points.len() < 2 {
            return String::new();
        }

        // 使用 Catmull-Rom 样条生成平滑曲线
        let mut path = format!("M {:.1},{:.1}", points[0].0, points[0].1);

        for i in 0..points.len() - 1 {
            let p0 = if i > 0 { points[i - 1] } else { points[i] };
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = if i < points.len() - 2 {
                points[i + 2]
            } else {
                points[i + 1]
            };

            // 计算控制点（使用 Catmull-Rom 转 Bezier 公式）
            let tension = 0.3; // 张力系数，越小越平滑
            let cp1x = p1.0 + (p2.0 - p0.0) * tension;
            let cp1y = p1.1 + (p2.1 - p0.1) * tension;
            let cp2x = p2.0 - (p3.0 - p1.0) * tension;
            let cp2y = p2.1 - (p3.1 - p1.1) * tension;

            path.push_str(&format!(
                " C {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
                cp1x, cp1y, cp2x, cp2y, p2.0, p2.1
            ));
        }

        path
    };

    let cpu_path = make_smooth_path(|s| s.cpu_usage);
    let mem_path = make_smooth_path(|s| {
        if s.total_memory > 0 {
            (s.memory_usage as f32 / s.total_memory as f32) * 100.0
        } else {
            0.0
        }
    });
    let gpu_path = make_smooth_path(|s| s.gpu_usage.map(|v| v as f32).unwrap_or(0.0));

    rsx! {
        div { class: "chart-container",
            div { class: "chart-header",
                h3 { class: "chart-title", "系统趋势" }

                div { class: "chart-controls",
                    for window in [TimeWindow::OneMin, TimeWindow::FiveMin, TimeWindow::All] {
                        button {
                            class: if time_window() == window { "chart-btn active" } else { "chart-btn" },
                            onclick: move |_| time_window.set(window),
                            "{window.label()}"
                        }
                    }
                }

                div { class: "chart-legend",
                    div { class: "legend-item",
                        span { class: "legend-dot legend-cpu" }
                        span { "CPU" }
                    }
                    div { class: "legend-item",
                        span { class: "legend-dot legend-mem" }
                        span { "内存" }
                    }
                    div { class: "legend-item",
                        span { class: "legend-dot legend-gpu" }
                        span { "GPU" }
                    }
                }
            }
            div { class: "chart-body",
                svg {
                    view_box: "0 0 100 100",
                    preserve_aspect_ratio: "none",
                    class: "chart-svg",

                    // 网格线
                    line {
                        x1: "0",
                        y1: "25",
                        x2: "100",
                        y2: "25",
                        class: "grid-line",
                    }
                    line {
                        x1: "0",
                        y1: "50",
                        x2: "100",
                        y2: "50",
                        class: "grid-line",
                    }
                    line {
                        x1: "0",
                        y1: "75",
                        x2: "100",
                        y2: "75",
                        class: "grid-line",
                    }

                    // 数据线
                    path {
                        d: "{gpu_path}",
                        class: "chart-line line-gpu",
                        fill: "none",
                        stroke_width: "3",
                        vector_effect: "non-scaling-stroke",
                    }
                    path {
                        d: "{mem_path}",
                        class: "chart-line line-mem",
                        fill: "none",
                        stroke_width: "3",
                        vector_effect: "non-scaling-stroke",
                    }
                    path {
                        d: "{cpu_path}",
                        class: "chart-line line-cpu",
                        fill: "none",
                        stroke_width: "3",
                        vector_effect: "non-scaling-stroke",
                    }
                }
            }
        }
    }
}
