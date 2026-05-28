use novel_core::{AgentEngine, EngineConfig, Op, TerminalReason};
use novel_config::{global_api_config_path, skills_dir, works_dir};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub agent_root: PathBuf,
    pub works_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub active_project: PathBuf,
    pub global_config_path: PathBuf,
}

impl AppConfig {
    pub fn from_agent_root(agent_root: PathBuf) -> Self {
        Self {
            works_dir: works_dir(&agent_root),
            skills_dir: skills_dir(&agent_root),
            global_config_path: global_api_config_path(&agent_root),
            active_project: works_dir(&agent_root).join("default"),
            agent_root,
        }
    }

    pub fn engine_config(&self) -> EngineConfig {
        self.engine_config_for(&self.active_project)
    }

    pub fn engine_config_for(&self, project: &Path) -> EngineConfig {
        EngineConfig {
            project_root: project.to_path_buf(),
            settings_path: project.join("settings.json"),
            db_path: project.join(".novel-agent/state.db"),
            skills_dir: self.skills_dir.clone(),
            global_config_path: self.global_config_path.clone(),
        }
    }

    pub fn active_work_name(&self) -> String {
        self.active_project
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "default".into())
    }

    pub fn templates_dir(&self) -> PathBuf {
        novel_config::templates_dir(&self.agent_root)
    }

    pub fn set_active_project(&mut self, project: PathBuf) {
        self.active_project = project;
    }

    /// Canonical accessor for the active project root path.
    pub fn project_root(&self) -> &Path {
        &self.active_project
    }

    pub fn db_path(&self) -> PathBuf {
        self.active_project.join(".novel-agent/state.db")
    }
}

pub struct NovelApp {
    engine: AgentEngine,
}

impl NovelApp {
    pub fn open(config: AppConfig) -> Result<Self, novel_core::AgentError> {
        novel_logging::init_logging(Some(config.active_project.as_path()));
        let engine = AgentEngine::new(config.engine_config())?;
        Ok(Self { engine })
    }

    pub async fn send_message(
        &mut self,
        content: &str,
    ) -> Result<TerminalReason, novel_core::AgentError> {
        self.engine.handle_message(content).await
    }

    pub fn session_id(&self) -> &str {
        &self.engine.shared.session.id
    }

    pub async fn run_loop(
        self,
        op_rx: mpsc::UnboundedReceiver<Op>,
        event_tx: mpsc::UnboundedSender<novel_core::Event>,
    ) -> Result<TerminalReason, novel_core::AgentError> {
        self.engine.run(op_rx, event_tx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn open_and_send_message() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut cfg = AppConfig::from_agent_root(tmp.path().to_path_buf());
        cfg.active_project = tmp.path().to_path_buf();
        let mut app = NovelApp::open(cfg).unwrap();
        assert!(!app.session_id().is_empty());
        let reason = app.send_message("测试消息").await.unwrap();
        assert_eq!(reason, TerminalReason::Completed);
    }
}
