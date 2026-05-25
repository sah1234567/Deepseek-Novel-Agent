mod app;

pub use app::{AppConfig, NovelApp};

#[cfg(feature = "tauri")]
pub mod tauri;
