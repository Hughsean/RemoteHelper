//! views 模块包含应用所有布局和路由的组件。
//! 每个视图对应一个路由页面。

mod dashboard;
mod home;
mod login;

pub use dashboard::Dashboard;
pub use login::LoginView;
