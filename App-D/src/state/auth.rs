//! 认证状态管理
//!
//! 符合 Dioxus 0.7 官方推荐的状态管理模式：
//! - 使用普通结构体存储数据
//! - 在组件中通过 use_signal 创建响应式状态
//! - 通过 use_context 访问共享状态

use dioxus::prelude::*;

/// 认证数据（非响应式）
#[derive(Clone, PartialEq, Debug)]
pub struct AuthData {
    /// 是否已通过服务器认证
    pub authenticated: bool,
    /// 是否已连接到服务器
    pub connected: bool,
    /// 错误消息
    pub error_msg: Option<String>,
    /// 服务器地址
    pub server_addr: String,
}

impl Default for AuthData {
    fn default() -> Self {
        // 从本地配置读取默认地址（可选）
        let default_addr =
            std::env::var("REMOTE_SERVER_ADDR").unwrap_or_else(|_| "frp-try.com:53460".to_string());

        Self {
            authenticated: false,
            connected: false,
            error_msg: None,
            server_addr: default_addr,
        }
    }
}

impl AuthData {
    /// 设置认证成功状态
    pub fn set_authenticated(&mut self) {
        self.authenticated = true;
        self.connected = true;
        self.error_msg = None;
    }

    /// 设置认证失败状态
    pub fn set_auth_failed(&mut self, error: String) {
        self.authenticated = false;
        self.connected = false;
        self.error_msg = Some(error);
    }

    /// 设置连接断开状态
    pub fn set_disconnected(&mut self, error: String) {
        self.connected = false;
        self.error_msg = Some(error);
    }

    /// 清除错误消息
    pub fn clear_error(&mut self) {
        self.error_msg = None;
    }

    /// 登出
    pub fn logout(&mut self) {
        self.authenticated = false;
        self.connected = false;
        self.error_msg = None;
    }
}

/// 认证状态的响应式包装（符合官方推荐）
///
/// 在组件中使用：
/// ```rust
/// let auth = use_auth_state();
/// if auth.read().authenticated { /* ... */ }
/// auth.write().set_authenticated();
/// ```
pub type AuthState = Signal<AuthData>;

/// Hook: 获取认证状态
///
/// 这是官方推荐的状态访问方式
pub fn use_auth_state() -> AuthState {
    use_context::<AuthState>()
}
