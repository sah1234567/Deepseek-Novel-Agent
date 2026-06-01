#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod app;

pub use app::{AppConfig, NovelApp};

#[cfg(feature = "tauri")]
pub mod tauri;
