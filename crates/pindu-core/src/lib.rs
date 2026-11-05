//! GTK-free core of Pindu launcher.
//!
//! Sources, launching, playtime and storage live here so they can be tested with plain
//! `cargo test` against fixture trees, without a display.

pub mod art;
pub mod art_cache;
pub mod backup;
pub mod focus;
pub mod gamepad;
pub mod grid;
pub mod hints;
pub mod launch;
pub mod launch_overlay;
pub mod library;
pub mod logs;
pub mod manual;
pub mod menus;
pub mod model;
pub mod osk;
pub mod panels;
pub mod paths;
pub mod pe;
pub mod permissions;
pub mod proton;
pub mod proton_downloads;
pub mod search;
pub mod sound;
pub mod sources;
pub mod steamgriddb;
pub mod store;
pub mod umu;
