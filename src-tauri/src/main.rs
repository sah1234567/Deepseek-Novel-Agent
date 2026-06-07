mod commands;
mod events;
mod state;

use novel_config::resolve_agent_root;
use novel_server::AppConfig;
use state::setup_app_state;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let agent_root = resolve_agent_root();
            std::fs::create_dir_all(novel_config::works_dir(&agent_root)).ok();
            std::fs::create_dir_all(novel_config::global_config_dir(&agent_root)).ok();

            let config = AppConfig::from_agent_root(agent_root);
            let templates_dir = config.templates_dir();
            if !templates_dir.is_dir() {
                return Err(
                    format!("scaffold templates not found: {}", templates_dir.display()).into(),
                );
            }
            if !config.active_project.exists() {
                novel_knowledge::init_project_scaffold(
                    &config.active_project,
                    templates_dir.as_path(),
                )
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            }
            if let Some(parent) = config.db_path().parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::create_dir_all(&config.skills_dir).ok();

            let app_state = setup_app_state(config)?;
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::interrupt,
            commands::approve_tool,
            commands::deny_tool,
            commands::answer_question,
            commands::get_app_status,
            commands::set_permission_mode,
            commands::init_novel_project,
            commands::create_session,
            commands::create_work,
            commands::open_work,
            commands::list_works,
            commands::resume_session,
            commands::get_fork_messages,
            commands::get_session_transcript_layout,
            commands::get_session_message_turns,
            commands::get_session_archive_turns,
            commands::list_sessions,
            commands::list_project_files,
            commands::read_project_file,
            commands::update_session_todo,
            commands::get_api_config,
            commands::set_api_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
