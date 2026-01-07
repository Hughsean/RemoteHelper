//! The components module contains all shared components for our app. Components are the building blocks of dioxus apps.
//! They can be used to defined common UI elements like buttons, forms, and modals. In this template, we define a Hero
//! component and an Echo component for fullstack apps to be used in our app.

mod hero;
pub use hero::Hero;

mod echo;
pub use echo::Echo;
pub mod hover_card;
pub mod card;
pub mod navbar;
pub mod dropdown_menu;
pub mod dialog;
pub mod scroll_area;
pub mod tooltip;
pub mod calendar;
pub mod date_picker;
pub mod toggle_group;
pub mod radio_group;
pub mod select;
pub mod aspect_ratio;
pub mod skeleton;
pub mod form;
pub mod toggle;
pub mod context_menu;
pub mod popover;
pub mod separator;
pub mod slider;
pub mod tabs;
pub mod toast;
pub mod collapsible;
pub mod checkbox;
pub mod button;
pub mod accordion;
pub mod avatar;
pub mod input;
pub mod toolbar;
pub mod sheet;
pub mod label;
pub mod menubar;
pub mod alert_dialog;
pub mod progress;
pub mod switch;
pub mod textarea;
