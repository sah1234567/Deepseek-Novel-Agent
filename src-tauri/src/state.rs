use novel_server::tauri::AppState;

pub use novel_server::AppConfig;

pub fn setup_app_state(config: AppConfig) -> Result<AppState, Box<dyn std::error::Error>> {
    if let Some(parent) = config.db_path().parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&config.skills_dir)?;
    AppState::new(config).map_err(|e| e.into())
}
