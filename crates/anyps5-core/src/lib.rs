//! UI-free logic of the AnyPS5 launcher. The launcher binary depends on this crate; it never depends on the UI.

pub mod controls;
pub mod diagnosis;
pub mod error;
pub mod events;
pub mod fsx;
pub mod hostcpu;
pub mod launch;
pub mod library;
pub mod patches;
pub mod paths;
pub mod process;
pub mod report;
pub mod settings;
pub mod toolkit;
pub mod trophies;
pub mod unlock;
pub mod unwrap;
#[cfg(windows)]
pub mod win;
