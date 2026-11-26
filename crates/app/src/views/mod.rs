//! 登录、监控、服务、设置及趋势图页面。

pub(crate) mod dashboard;
pub(crate) mod history_settings;
pub(crate) mod login;
pub(crate) mod services;
pub(crate) mod settings;
pub(crate) mod trend;

pub(crate) use dashboard::Dashboard;
pub(crate) use login::Login;
pub(crate) use services::Services;
