//! GTK4 and libadwaita shells of Pindu launcher. The binary in `main.rs` only sets up
//! gettext and resources and runs [`application::PinduApplication`].

pub mod application;
pub mod collection_actions;
pub mod config;
pub mod file_service;
pub mod game_dialog;
pub mod game_page;
pub mod gamepad_service;
pub mod launch_service;
pub mod library_service;
pub mod manette_backend;
pub mod models;
pub mod permission_service;
pub mod preferences;
pub mod search_provider;
pub mod session_service;
pub mod sources;
pub mod steamgriddb_picker;
pub mod steamgriddb_service;
pub mod sync_service;
pub mod watch_service;
pub mod widgets;
pub mod window;
