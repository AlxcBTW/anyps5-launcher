//! UI-free logic of the AnyPS5 launcher. The launcher binary depends on this crate; it never depends on the UI.

pub mod community;
pub mod controls;
pub mod diagnosis;
pub mod discord;
pub mod error;
pub mod events;
pub mod fsx;
pub mod gamefiles;
pub mod hostcpu;
pub mod issue;
pub mod launch;
pub mod library;
pub mod linkreg;
pub mod links;
pub mod patches;
pub mod paths;
pub mod process;
pub mod report;
pub mod selfupdate;
pub mod settings;
pub mod toolkit;
pub mod trophies;
pub mod unlock;
pub mod unwrap;
#[cfg(windows)]
pub mod win;
