use chrono::Utc;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum LogEvent {
    SessionCreated {
        session_id: String,
        project_root: String,
        model: String,
    },
    TurnStarted {
        session_id: String,
        turn_number: u32,
        message_count: usize,
    },
    TurnCompleted {
        session_id: String,
        turn_number: u32,
        cache_hit_tokens: i64,
        cache_miss_tokens: i64,
        completion_tokens: i64,
    },
    LlmRequest {
        session_id: String,
        model: String,
        streaming: bool,
    },
    TokenAudit {
        session_id: String,
        cache_hit_tokens: i64,
        cache_miss_tokens: i64,
        completion_tokens: i64,
    },
    ToolExecuted {
        session_id: String,
        tool_name: String,
        success: bool,
    },
    KnowledgeAuditorHookForked {
        session_id: String,
        trigger_tool: String,
    },
    CompactionTriggered {
        session_id: String,
        level: String,
        tokens_before: usize,
    },
    Error {
        message: String,
        recoverable: bool,
    },
}

pub struct AuditLogger {
    agent_log: Arc<Mutex<File>>,
    token_log: Arc<Mutex<File>>,
    replay_log: Arc<Mutex<File>>,
    session_id: String,
}

impl AuditLogger {
    pub fn open(project_root: &Path, session_id: &str) -> std::io::Result<Self> {
        let log_dir = project_root
            .join(".novel/logs")
            .join(format!("session_{session_id}"));
        std::fs::create_dir_all(&log_dir)?;
        let agent_log = open_append(log_dir.join("agent.jsonl"))?;
        let token_log = open_append(log_dir.join("token_audit.jsonl"))?;
        let replay_log = open_append(log_dir.join("replay.jsonl"))?;
        Ok(Self {
            agent_log: Arc::new(Mutex::new(agent_log)),
            token_log: Arc::new(Mutex::new(token_log)),
            replay_log: Arc::new(Mutex::new(replay_log)),
            session_id: session_id.to_string(),
        })
    }

    pub fn log(&self, event: &LogEvent) -> std::io::Result<()> {
        let line = serde_json::to_string(event)? + "\n";
        let mut f = self.agent_log.lock().unwrap_or_else(|e| e.into_inner());
        f.write_all(line.as_bytes())?;
        f.flush()?;
        if matches!(event, LogEvent::TokenAudit { .. }) {
            let mut t = self.token_log.lock().unwrap_or_else(|e| e.into_inner());
            t.write_all(line.as_bytes())?;
            t.flush()?;
        }
        Ok(())
    }

    pub fn replay(&self, role: &str, content: &str) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "ts": Utc::now().to_rfc3339(),
            "session_id": self.session_id,
            "role": role,
            "content": content,
        });
        let mut f = self.replay_log.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(f, "{}", entry)?;
        f.flush()
    }
}

fn open_append(path: PathBuf) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn audit_logger_writes_jsonl() {
        let tmp = TempDir::new().unwrap();
        let logger = AuditLogger::open(tmp.path(), "test-session").unwrap();
        logger
            .log(&LogEvent::TurnStarted {
                session_id: "test-session".into(),
                turn_number: 1,
                message_count: 3,
            })
            .unwrap();
        let path = tmp
            .path()
            .join(".novel/logs/session_test-session/agent.jsonl");
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("TurnStarted"));
    }
}
