use crate::components::card::*;
use crate::components::progress::*;
use crate::components::separator::*;
use dioxus::prelude::*;

const METRIC_CARD_CSS: Asset = asset!("/assets/components/metric_card/style.css");

/// 单个指标项
#[derive(Clone, PartialEq)]
pub struct MetricItem {
    pub label: String,
    pub value: String,
    pub progress: Option<f32>, // 0.0 - 1.0，None 表示不显示进度条
}

/// 指标卡片数据
#[derive(Clone, PartialEq)]
pub struct MetricCardData {
    pub title: String,
    pub main_value: String,
    pub items: Vec<MetricItem>,
    pub footer: Option<String>,
}

/// 指标卡片组件 - 用于显示系统监控数据
#[component]
pub fn MetricCard(
    /// 卡片数据（支持响应式更新）
    data: ReadSignal<MetricCardData>,
) -> Element {
    let card_data = data();

    rsx! {
        Card { class: "metric-card",
            document::Link { rel: "stylesheet", href: METRIC_CARD_CSS }

            CardHeader {
                CardTitle { class: "metric-card-title", "{card_data.title}" }
                div { class: "metric-card-main-value", "{card_data.main_value}" }
            }

            CardContent { class: "metric-card-items",
                for item in card_data.items {
                    div { class: "metric-item",
                        div { class: "metric-item-header",
                            span { class: "metric-item-label", "{item.label}" }
                            span { class: "metric-item-value", "{item.value}" }
                        }
                        if let Some(progress) = item.progress {
                            Progress {
                                class: "metric-progress-bar",
                                value: progress as f64,
                                max: 1.0,
                                ProgressIndicator { class: "metric-progress-fill" }
                            }
                        }
                    }
                }
            }

            if let Some(footer_text) = card_data.footer {
                Separator { horizontal: true, decorative: true }
                CardFooter { class: "metric-card-footer", "{footer_text}" }
            }
        }
    }
}
