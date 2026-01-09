//! The views module contains the components for all Layouts and Routes for our app.
//!
//! Currently contains the Login and Dashboard views.

mod login;
pub use login::Login;

mod home;
pub use home::Home;

mod test;
pub use test::Test;

mod services;
pub use services::Services;
