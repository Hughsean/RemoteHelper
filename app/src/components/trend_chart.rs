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

    // 辅助函数：将数据转换为 SVG polyline points 字符串
    let make_points = |extractor: fn(&SystemInfo) -> f32| -> String {
        if filtered_history.is_empty() {
            return String::new();
        }
        filtered_history
            .iter()
            .enumerate()
            .map(|(i, info)| {
                let x = (i as f32 / (filtered_history.len().max(2) - 1) as f32) * width;
                let val = extractor(info).clamp(0.0, 100.0);
                let y = height - (val / 100.0 * height);
                format!("{:.1},{:.1}", x, y)
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let cpu_points = make_points(|s| s.cpu_usage);
    let mem_points = make_points(|s| {
        if s.total_memory > 0 {
            (s.memory_usage as f32 / s.total_memory as f32) * 100.0
        } else {
            0.0
        }
    });
    let gpu_points = make_points(|s| s.gpu_usage.map(|v| v as f32).unwrap_or(0.0));

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
                    polyline {
                        points: "{gpu_points}",
                        class: "chart-line line-gpu",
                        fill: "none",
                        stroke_width: "3",
                        vector_effect: "non-scaling-stroke",
                    }
                    polyline {
                        points: "{mem_points}",
                        class: "chart-line line-mem",
                        fill: "none",
                        stroke_width: "3",
                        vector_effect: "non-scaling-stroke",
                    }
                    polyline {
                        points: "{cpu_points}",
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
