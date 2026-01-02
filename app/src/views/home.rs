//! 主页面 - Dashboard
//!
//! 整合所有组件，展示完整的监控界面

use dioxus::prelude::*;
use crate::components::{Header, SystemStatusDisplay, TrendChart, ServiceList, Login, AddServiceModal};
use crate::state::{use_auth_state, use_system_state, use_services_state};
use crate::backend;
use std::time::Duration;

#[component]
pub fn Home() -> Element {
    let mut auth = use_auth_state();
    let mut system = use_system_state();
    let mut services = use_services_state();

    // 如果未认证，显示登录界面
    if !auth.read().authenticated {
        return rsx! {
            Login {}
        };
    }

    // 自动刷新数据
    use_resource(move || {
        async move {
            loop {
                let interval = system.read().refresh_interval;

                // 如果间隔为0，暂停刷新
                if interval == 0 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }

                // 获取系统状态
                if let Ok(status) = backend::get_status(Some(interval)).await {
                    system.write().update_status(status);
                }

                // 获取服务列表
                if let Ok(services_list) = backend::list_services().await {
                    services.write().update_services(services_list);
                }

                tokio::time::sleep(Duration::from_millis(interval)).await;
            }
        }
    });

    let system_data = system.read();
    let services_data = services.read();

    rsx! {
        div { class: "app-container",
            Header {}

            div { class: "main-content",
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
        }
    }
}

