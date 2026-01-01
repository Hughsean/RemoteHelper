use std::collections::VecDeque;

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
            TimeWindow::All => "30分钟",
        }
    }
}

#[component]
pub fn TrendChart(history: VecDeque<(u64, SystemInfo)>) -> Element {
    let mut time_window = use_signal(|| TimeWindow::OneMin);
    let mut show_cpu = use_signal(|| true);
    let mut show_mem = use_signal(|| true);
    let mut show_gpu = use_signal(|| true);

    if history.is_empty() {
        return rsx! {};
    }

    // Use the latest timestamp from data instead of client time to avoid clock skew
    let latest_timestamp = history.back().map(|(t, _)| *t).unwrap_or(0);
    let filtered_history: Vec<(u64, &SystemInfo)> = history
        .iter()
        .filter(|(t, _)| match time_window() {
            TimeWindow::OneMin => *t > latest_timestamp.saturating_sub(60 * 1000),
            TimeWindow::FiveMin => *t > latest_timestamp.saturating_sub(5 * 60 * 1000),
            TimeWindow::All => true,
        })
        .map(|(t, s)| (*t, s))
        .collect();

    let width = 100.0;
    let height = 100.0;
    let margin_top = 5.0;
    let margin_bottom = 5.0;
    let chart_height = height - margin_top - margin_bottom;

    // 收集所有启用的数据线的数值，用于统一缩放
    let mut all_values: Vec<f32> = Vec::new();
    if show_cpu() {
        all_values.extend(
            filtered_history
                .iter()
                .map(|(_, info)| info.cpu_usage.clamp(0.0, 100.0)),
        );
    }
    if show_mem() {
        all_values.extend(filtered_history.iter().map(|(_, info)| {
            if info.total_memory > 0 {
                ((info.memory_usage as f32 / info.total_memory as f32) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            }
        }));
    }
    if show_gpu() {
        all_values.extend(
            filtered_history
                .iter()
                .filter_map(|(_, info)| info.gpu_usage.map(|v| (v as f32).clamp(0.0, 100.0))),
        );
    }

    // 计算统一的缩放范围（类似 Windows 任务管理器的动态缩放）
    let (scale_min, scale_max) = if !all_values.is_empty() {
        let min_val = all_values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_val = all_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        // 动态范围，最小10%以保证可见变化
        let range = (max_val - min_val).max(10.0);
        let center = (max_val + min_val) / 2.0;
        let calc_min = (center - range / 2.0).max(0.0);
        let calc_max = (center + range / 2.0).min(100.0);

        (calc_min, calc_max)
    } else {
        (0.0, 100.0)
    };

    // 辅助函数：生成折线 SVG path 字符串（基于时间戳）
    let make_line_path = |extractor: fn(&SystemInfo) -> f32| -> String {
        if filtered_history.is_empty() {
            return String::new();
        }

        let oldest_timestamp = filtered_history.first().map(|(t, _)| *t).unwrap_or(0);
        let time_range = latest_timestamp.saturating_sub(oldest_timestamp).max(1);

        let mut path = String::new();
        for (timestamp, info) in &filtered_history {
            let val = extractor(info).clamp(0.0, 100.0);

            // 基于时间戳计算 x 坐标，而不是索引
            let x = ((*timestamp - oldest_timestamp) as f32 / time_range as f32) * width;
            let normalized = if scale_max > scale_min {
                (val - scale_min) / (scale_max - scale_min)
            } else {
                0.5
            };
            let y = margin_top + (1.0 - normalized) * chart_height;

            if path.is_empty() {
                path.push_str(&format!("M {:.1},{:.1}", x, y));
            } else {
                path.push_str(&format!(" L {:.1},{:.1}", x, y));
            }
        }

        path
    };

    let cpu_path = make_line_path(|s| s.cpu_usage);
    let mem_path = make_line_path(|s| {
        if s.total_memory > 0 {
            (s.memory_usage as f32 / s.total_memory as f32) * 100.0
        } else {
            0.0
        }
    });
    let gpu_path = make_line_path(|s| s.gpu_usage.map(|v| v as f32).unwrap_or(0.0));

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
                    div {
                        class: if show_cpu() { "legend-item" } else { "legend-item legend-item-disabled" },
                        onclick: move |_| show_cpu.set(!show_cpu()),
                        span { class: "legend-dot legend-cpu" }
                        span { "CPU" }
                    }
                    div {
                        class: if show_mem() { "legend-item" } else { "legend-item legend-item-disabled" },
                        onclick: move |_| show_mem.set(!show_mem()),
                        span { class: "legend-dot legend-mem" }
                        span { "内存" }
                    }
                    div {
                        class: if show_gpu() { "legend-item" } else { "legend-item legend-item-disabled" },
                        onclick: move |_| show_gpu.set(!show_gpu()),
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
                    if show_gpu() {
                        path {
                            d: "{gpu_path}",
                            class: "chart-line line-gpu",
                            fill: "none",
                            stroke_width: "1.5",
                            vector_effect: "non-scaling-stroke",
                        }
                    }
                    if show_mem() {
                        path {
                            d: "{mem_path}",
                            class: "chart-line line-mem",
                            fill: "none",
                            stroke_width: "1.5",
                            vector_effect: "non-scaling-stroke",
                        }
                    }
                    if show_cpu() {
                        path {
                            d: "{cpu_path}",
                            class: "chart-line line-cpu",
                            fill: "none",
                            stroke_width: "1.5",
                            vector_effect: "non-scaling-stroke",
                        }
                    }
                }
            }
        }
    }
}
