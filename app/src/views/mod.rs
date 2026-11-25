//! 页面层：登录页、监控页、服务页，以及监控页使用的趋势图。

pub(crate) mod dashboard;
pub(crate) mod login;
pub(crate) mod services;
pub(crate) mod trend;

pub(crate) use dashboard::Dashboard;
pub(crate) use login::Login;
pub(crate) use services::Services;
