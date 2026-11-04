//! GTK4 and libadwaita shells of Pindu launcher. The binary in `main.rs` only sets up
//! gettext and resources and runs [`application::PinduApplication`].

pub mod application;
pub mod config;
pub mod game_page;
pub mod library_service;
pub mod models;
pub mod sources;
pub mod sync_service;
pub mod widgets;
pub mod window;
