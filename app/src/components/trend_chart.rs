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

    // 使用 use_memo 缓存过滤后的数据，只在 history 或 time_window 变化时重新计算
    let chart_data = use_memo(move || {
        let latest_timestamp = history.back().map(|(t, _)| *t).unwrap_or(0);
        let time_cutoff = match time_window() {
            TimeWindow::OneMin => latest_timestamp.saturating_sub(60 * 1000),
            TimeWindow::FiveMin => latest_timestamp.saturating_sub(5 * 60 * 1000),
            TimeWindow::All => 0,
        };

        // 直接使用迭代器而不是 collect，减少分配
        let filtered: Vec<_> = history.iter().filter(|(t, _)| *t > time_cutoff).collect();

        if filtered.is_empty() {
            return (String::new(), String::new(), String::new(), 0.0, 100.0);
        }

        let oldest_timestamp = filtered.first().map(|(t, _)| *t).unwrap_or(0);
        let time_range = latest_timestamp.saturating_sub(oldest_timestamp).max(1);

        // 一次遍历收集所有数据和计算缩放范围
        let mut cpu_values = Vec::with_capacity(filtered.len());
        let mut mem_values = Vec::with_capacity(filtered.len());
        let mut gpu_values = Vec::with_capacity(filtered.len());
        let mut all_values = Vec::with_capacity(filtered.len() * 3);

        for (timestamp, info) in &filtered {
            let cpu_val = info.cpu_usage.clamp(0.0, 100.0);
            let mem_val = if info.total_memory > 0 {
                ((info.memory_usage as f32 / info.total_memory as f32) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let gpu_val = info.gpu_usage.map(|v| (v as f32).clamp(0.0, 100.0));

            cpu_values.push((*timestamp, cpu_val));
            mem_values.push((*timestamp, mem_val));
            if let Some(gv) = gpu_val {
                gpu_values.push((*timestamp, gv));
                all_values.push(gv);
            }

            all_values.push(cpu_val);
            all_values.push(mem_val);
        }

        // 计算统一的缩放范围
        let (scale_min, scale_max) = if !all_values.is_empty() {
            let min_val = all_values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = all_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let range = (max_val - min_val).max(10.0);
            let center = (max_val + min_val) / 2.0;
            let calc_min = (center - range / 2.0).max(0.0);
            let calc_max = (center + range / 2.0).min(100.0);
            (calc_min, calc_max)
        } else {
            (0.0, 100.0)
        };

        const WIDTH: f32 = 100.0;
        const HEIGHT: f32 = 100.0;
        const MARGIN_TOP: f32 = 5.0;
        const MARGIN_BOTTOM: f32 = 5.0;
        const CHART_HEIGHT: f32 = HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;

        // 生成 SVG 路径的辅助函数
        let make_path = |values: &[(u64, f32)]| -> String {
            if values.is_empty() {
                return String::new();
            }

            // 预估路径长度以减少重新分配
            let mut path = String::with_capacity(values.len() * 20);
            for (i, &(timestamp, val)) in values.iter().enumerate() {
                let x = ((timestamp - oldest_timestamp) as f32 / time_range as f32) * WIDTH;
                let normalized = if scale_max > scale_min {
                    (val - scale_min) / (scale_max - scale_min)
                } else {
                    0.5
                };
                let y = MARGIN_TOP + (1.0 - normalized) * CHART_HEIGHT;

                if i == 0 {
                    path.push_str(&format!("M {:.1},{:.1}", x, y));
                } else {
                    path.push_str(&format!(" L {:.1},{:.1}", x, y));
                }
            }
            path
        };

        let cpu_path = make_path(&cpu_values);
        let mem_path = make_path(&mem_values);
        let gpu_path = make_path(&gpu_values);

        (cpu_path, mem_path, gpu_path, scale_min, scale_max)
    });

    let (cpu_path, mem_path, gpu_path, _scale_min, _scale_max) = chart_data();

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
