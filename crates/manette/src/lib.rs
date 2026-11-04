//! Rust bindings for libmanette 0.2, the GNOME game controller library. `auto` is generated
//! by gir from `gir/Manette-0.2.gir` (see `Gir.toml`); `Event` and `Monitor::devices` are
//! written by hand, since gir skips them.

pub use ffi;
pub use glib;

// gir imports glib's prelude in every generated file, whether it is used or not, as in
// gtk-rs's own crates.
#[allow(unused_imports)]
mod auto;
pub use auto::*;

mod event;
pub use event::Event;

mod monitor;
