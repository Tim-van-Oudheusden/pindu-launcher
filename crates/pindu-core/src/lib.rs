//! GTK-free core of Pindu launcher.
//!
//! Sources, launching, playtime and storage live here so they can be tested with plain
//! `cargo test` against fixture trees, without a display.

pub mod model;
pub mod paths;
pub mod store;
