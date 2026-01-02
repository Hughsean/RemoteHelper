//! 状态管理模块
//!
//! 符合 Dioxus 0.7 官方推荐的状态管理模式：
//!
//! 1. **数据层**: 普通结构体存储应用数据（AuthData, SystemData, ServicesData）
//! 2. **响应式层**: Signal 包装数据使其响应式（AuthState, SystemState, ServicesState）
//! 3. **访问层**: 自定义 hooks 访问状态（use_auth_state, use_system_state, use_services_state）
//!
//! ## 使用示例
//!
//! ```rust
//! #[component]
//! fn MyComponent() -> Element {
//!     // 获取认证状态
//!     let auth = use_auth_state();
//!
//!     // 读取状态
//!     if auth.read().authenticated {
//!         // 已认证逻辑
//!     }
//!
//!     // 修改状态
//!     auth.write().set_authenticated();
//!
//!     rsx! { /* ... */ }
//! }
//! ```

mod auth;
mod services;
mod system;

// 导出数据结构
pub use auth::AuthData;
pub use services::ServicesData;
pub use system::SystemData;

// 导出自定义 hooks
pub use auth::use_auth_state;
pub use services::use_services_state;
pub use system::use_system_state;
