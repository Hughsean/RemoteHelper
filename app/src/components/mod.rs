//! The components module contains all shared components for our app. Components are the building blocks of dioxus apps.
//! This module provides reusable UI components.

mod metric_card;
pub use metric_card::{MetricCard, MetricCardData, MetricItem};

mod trend_chart;
pub use trend_chart::{DataSeries, TrendChart, TrendChartData};

mod add_service_dialog;
pub use add_service_dialog::AddServiceDialog;

pub mod calendar;
pub mod card;
pub mod date_picker;
pub mod dialog;
pub mod dropdown_menu;
pub mod hover_card;
pub mod radio_group;
pub mod scroll_area;
pub mod select;
pub mod toggle_group;
pub mod tooltip;
pub use select::*;
pub mod accordion;
pub mod alert_dialog;
pub mod aspect_ratio;
pub mod avatar;
pub mod button;
pub mod checkbox;
pub mod collapsible;
pub mod context_menu;
pub mod form;
pub mod input;
pub mod label;
pub mod menubar;
pub mod popover;
pub mod progress;
pub mod separator;
pub mod sheet;
pub mod skeleton;
pub mod slider;
pub mod switch;
pub mod tabs;
pub mod textarea;
pub mod toast;
pub mod toggle;
pub mod toolbar;
