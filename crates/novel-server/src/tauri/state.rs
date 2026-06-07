use crate::AppConfig;
use novel_core::{AbortController, AgentEngine};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::RwLock;

use super::engine_loop::{spawn_engine_loop, EngineCommandTx};

pub struct CommandContext {
    pub config: Arc<RwLock<AppConfig>>,
    pub cmd_tx: EngineCommandTx,
    pub app_handle: AppHandle,
    pub current_message_id: Arc<RwLock<String>>,
    pub abort_controller: Arc<AbortController>,
    /// Mirrors engine turn activity for fast `set_permission_mode` rejection without queueing.
    pub turn_in_progress: Arc<AtomicBool>,
}

pub struct AppState {
    config: Arc<RwLock<AppConfig>>,
    cmd_tx: EngineCommandTx,
    current_message_id: Arc<RwLock<String>>,
    abort_controller: Arc<AbortController>,
    turn_in_progress: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        novel_logging::init_logging(Some(config.active_project.as_path()));
        let config = Arc::new(RwLock::new(config));
        let abort_controller = AbortController::shared();
        let turn_in_progress = Arc::new(AtomicBool::new(false));
        let engine = {
            let cfg = config.blocking_read();
            AgentEngine::new_with_abort(cfg.engine_config(), Arc::clone(&abort_controller))
                .map_err(|e| e.to_string())?
        };
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_engine_loop(
            engine,
            cmd_rx,
            Arc::clone(&config),
            Arc::clone(&abort_controller),
            Arc::clone(&turn_in_progress),
        );
        Ok(Self {
            config,
            cmd_tx,
            current_message_id: Arc::new(RwLock::new(String::new())),
            abort_controller,
            turn_in_progress,
        })
    }

    pub fn command_context(&self, app_handle: AppHandle) -> CommandContext {
        CommandContext {
            config: Arc::clone(&self.config),
            cmd_tx: self.cmd_tx.clone(),
            app_handle,
            current_message_id: Arc::clone(&self.current_message_id),
            abort_controller: Arc::clone(&self.abort_controller),
            turn_in_progress: Arc::clone(&self.turn_in_progress),
        }
    }

    pub fn config(&self) -> Arc<RwLock<AppConfig>> {
        Arc::clone(&self.config)
    }
}
