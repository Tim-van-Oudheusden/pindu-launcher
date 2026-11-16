//! Fullscreen mode's system panel (`docs/system-panel.md`): what lets the owner of a console
//! run the machine with a controller only. Each section has its pure logic and the trait
//! of the backend a UI service drives here, so every flow is tested with a fake backend.

pub mod network;
pub mod update;
