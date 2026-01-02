//! 服务管理状态
//!
//! 符合 Dioxus 0.7 官方推荐的状态管理模式

use dioxus::prelude::*;

/// 服务管理数据（非响应式）
#[derive(Clone, PartialEq, Debug)]
pub struct ServicesData {
    /// 服务列表
    pub services: Vec<common::ServiceInfo>,
    /// 是否显示添加服务模态框
    pub show_add_modal: bool,
    /// 当前操作中的服务ID（用于加载状态）
    pub operating_service: Option<usize>,
    /// 是否显示凭据输入窗口
    pub show_credential_modal: bool,
    /// 待执行的凭据操作 (service_id, action)
    pub pending_credential_action: Option<(usize, String)>,
    /// 凭据输入 - 用户名
    pub credential_username: String,
    /// 凭据输入 - 密码
    pub credential_password: String,
}

impl Default for ServicesData {
    fn default() -> Self {
        Self {
            services: Vec::new(),
            show_add_modal: false,
            operating_service: None,
            show_credential_modal: false,
            pending_credential_action: None,
            credential_username: String::new(),
            credential_password: String::new(),
        }
    }
}

impl ServicesData {
    /// 更新服务列表
    pub fn update_services(&mut self, services: Vec<common::ServiceInfo>) {
        self.services = services;
    }

    /// 显示添加服务模态框
    pub fn show_add_modal(&mut self) {
        self.show_add_modal = true;
    }

    /// 隐藏添加服务模态框
    pub fn hide_add_modal(&mut self) {
        self.show_add_modal = false;
    }

    /// 设置正在操作的服务
    pub fn set_operating(&mut self, service_id: usize) {
        self.operating_service = Some(service_id);
    }

    /// 清除操作状态
    pub fn clear_operating(&mut self) {
        self.operating_service = None;
    }

    /// 检查服务是否正在操作中
    pub fn is_operating(&self, service_id: usize) -> bool {
        self.operating_service == Some(service_id)
    }

    /// 根据ID查找服务
    pub fn find_service(&self, id: usize) -> Option<common::ServiceInfo> {
        self.services.iter().find(|s| s.id == id).cloned()
    }

    /// 显示凭据输入窗口
    pub fn show_credential_modal(&mut self, service_id: usize, action: String) {
        self.show_credential_modal = true;
        self.pending_credential_action = Some((service_id, action));
        self.credential_username = String::new();
        self.credential_password = String::new();
    }

    /// 隐藏凭据输入窗口
    pub fn hide_credential_modal(&mut self) {
        self.show_credential_modal = false;
        self.pending_credential_action = None;
        self.credential_username = String::new();
        self.credential_password = String::new();
    }

    /// 设置凭据用户名
    pub fn set_credential_username(&mut self, username: String) {
        self.credential_username = username;
    }

    /// 设置凭据密码
    pub fn set_credential_password(&mut self, password: String) {
        self.credential_password = password;
    }
}

/// 服务状态的响应式包装（符合官方推荐）
pub type ServicesState = Signal<ServicesData>;

/// Hook: 获取服务管理状态
pub fn use_services_state() -> ServicesState {
    use_context::<ServicesState>()
}
