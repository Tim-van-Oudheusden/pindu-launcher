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
pub mod launch;
pub mod library;
pub mod manual;
pub mod model;
pub mod paths;
pub mod permissions;
pub mod proton;
pub mod search;
pub mod sound;
pub mod sources;
pub mod steamgriddb;
pub mod store;
pub mod umu;
