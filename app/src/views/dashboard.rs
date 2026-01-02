//! Dashboard 视图
//!
//! 主监控面板 - 显示系统状态、趋势图表和服务列表

use crate::backend;
use crate::components::{
    AddServiceModal, CredentialModal, Header, ServiceList, SystemStatusDisplay, TrendChart,
};
use crate::state::{use_services_state, use_system_state};
use dioxus::prelude::*;

#[component]
pub fn Dashboard() -> Element {
    let mut system = use_system_state();
    let mut services = use_services_state();

    // 初始加载数据
    let _initial_load = use_resource(move || {
        async move {
            // 首次获取系统状态
            if let Ok(status) = backend::get_status(Some(1000)).await {
                system.write().update_status(status);
            }
            // 首次获取服务列表
            if let Ok(services_list) = backend::list_services().await {
                services.write().update_services(services_list);
            }
        }
    });

    // 定时刷新数据
    use_future(move || {
        async move {
            loop {
                let interval = system.read().refresh_interval;

                // 如果间隔为0，暂停刷新
                if interval == 0 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }

                // 等待间隔时间
                tokio::time::sleep(std::time::Duration::from_millis(interval)).await;

                // 获取系统状态
                match backend::get_status(Some(interval)).await {
                    Ok(status) => {
                        system.write().update_status(status);
                    }
                    Err(e) => {
                        dioxus_logger::tracing::error!("获取系统状态失败: {}", e);
                    }
                }

                // 获取服务列表
                match backend::list_services().await {
                    Ok(services_list) => {
                        services.write().update_services(services_list);
                    }
                    Err(e) => {
                        dioxus_logger::tracing::error!("获取服务列表失败: {}", e);
                    }
                }
            }
        }
    });

    let system_data = system.read();
    let services_data = services.read();

    rsx! {
        div { class: "dashboard-page",
            Header {}

            div { class: "dashboard-content",
                // 系统状态卡片
                if let Some(status) = &system_data.current_status {
                    SystemStatusDisplay { status: status.clone() }
                } else {
                    div { class: "loading-state", "正在加载系统状态..." }
                }

                // 趋势图表
                TrendChart { history: system_data.history.clone() }

                // 服务列表
                ServiceList {}
            }

            // 添加服务模态框
            if services_data.show_add_modal {
                AddServiceModal {}
            }

            // 凭据输入模态框
            if services_data.show_credential_modal {
                CredentialModal {}
            }
        }
    }
}
