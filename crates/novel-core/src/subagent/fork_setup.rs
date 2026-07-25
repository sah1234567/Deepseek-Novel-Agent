//! Fork initialization: build `ForkedAgentContext` and `[system, task]` message pair.

use crate::agent::{format_fork_task, SkillLoadRoots};
use crate::{AgentDefinition, AgentError, AgentType, ChatMessage};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ForkError {
    #[error("Invalid max react loops: {0}")]
    InvalidMaxReactLoops(u32),
    #[error("Empty task message")]
    EmptyTask,
    #[error("Knowledge file missing: {0}")]
    KnowledgeFileMissing(String),
}

/// Inputs for [`ForkedAgentContext::fork`].
pub struct ForkBuildParams<'a> {
    pub parent_system_message: &'a ChatMessage,
    pub parent_session_id: String,
    pub agent_type: AgentType,
    pub task_prompt: String,
    pub max_react_loops: u32,
    pub knowledge_snapshots: HashMap<PathBuf, String>,
    pub parent_is_forked: bool,
    pub skill_roots: Option<SkillLoadRoots<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationFork {
    pub parent_session_id: String,
    pub frozen_knowledge_snapshots: HashMap<PathBuf, String>,
    pub agent_def: AgentDefinition,
    pub task_message: ChatMessage,
    pub max_react_loops: u32,
}

impl ConversationFork {
    /// 构建子 agent 消息：仅 system prompt + task_message。
    /// system prompt 是每次 API 请求的固定前缀 → DeepSeek KV cache 必然命中。
    pub fn build_messages(&self, parent_system_message: &ChatMessage) -> Vec<ChatMessage> {
        vec![parent_system_message.clone(), self.task_message.clone()]
    }

    pub fn validate(&self) -> Result<(), ForkError> {
        if self.max_react_loops == 0 || self.max_react_loops > 80 {
            return Err(ForkError::InvalidMaxReactLoops(self.max_react_loops));
        }
        if self.task_message.content.trim().is_empty() {
            return Err(ForkError::EmptyTask);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub struct ForkedAgentContext {
    pub fork: ConversationFork,
    pub messages: Vec<ChatMessage>,
    pub react_loop_count: u32,
    pub is_child: bool,
}

impl ForkedAgentContext {
    pub fn fork(params: ForkBuildParams<'_>) -> Result<Self, ForkError> {
        validate_fork_request(params.parent_is_forked, params.max_react_loops)?;
        let fork = build_conversation_fork(&params)?;
        fork.validate()?;
        let messages = fork.build_messages(params.parent_system_message);
        Ok(Self {
            fork,
            messages,
            react_loop_count: 0,
            is_child: true,
        })
    }

    pub fn build_request_messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn increment_react_loop(&mut self) -> Result<(), ForkError> {
        self.react_loop_count += 1;
        if self.react_loop_count >= self.fork.max_react_loops {
            return Err(ForkError::InvalidMaxReactLoops(self.fork.max_react_loops));
        }
        Ok(())
    }
}

fn validate_fork_request(parent_is_forked: bool, max_react_loops: u32) -> Result<(), ForkError> {
    if parent_is_forked {
        // Mapped to NestedFork in engine.
        return Err(ForkError::InvalidMaxReactLoops(0));
    }
    if max_react_loops == 0 || max_react_loops > 80 {
        return Err(ForkError::InvalidMaxReactLoops(max_react_loops));
    }
    Ok(())
}

fn build_conversation_fork(params: &ForkBuildParams<'_>) -> Result<ConversationFork, ForkError> {
    let agent_def = params.agent_type.definition();
    let formatted_task = format_fork_task(
        params.agent_type,
        &params.task_prompt,
        &agent_def.tools,
        params.skill_roots,
    )
    .map_err(|_| ForkError::EmptyTask)?;
    Ok(ConversationFork {
        parent_session_id: params.parent_session_id.clone(),
        frozen_knowledge_snapshots: params.knowledge_snapshots.clone(),
        agent_def: agent_def.clone(),
        task_message: ChatMessage {
            role: "user".into(),
            content: formatted_task,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            ..Default::default()
        },
        max_react_loops: params.max_react_loops,
    })
}

/// Build a fork child context from engine state (shared system prompt + formatted task).
pub fn build_fork_child(
    shared: &crate::EngineShared,
    agent_type: AgentType,
    task: String,
) -> Result<ForkedAgentContext, AgentError> {
    let system_msg = ChatMessage {
        role: "system".into(),
        content: shared.system_prompt.clone(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        ..Default::default()
    };
    let roots = SkillLoadRoots {
        project_root: shared.session.project_root.as_path(),
        agent_skills_dir: Some(shared.agent_skills_dir.as_path()),
    };
    ForkedAgentContext::fork(ForkBuildParams {
        parent_system_message: &system_msg,
        parent_session_id: shared.session.id.clone(),
        agent_type,
        task_prompt: task,
        max_react_loops: agent_type.max_react_loops_for(&shared.settings.agent),
        knowledge_snapshots: HashMap::new(),
        parent_is_forked: false,
        skill_roots: Some(roots),
    })
    .map_err(|e| {
        if e == ForkError::InvalidMaxReactLoops(0) {
            AgentError::NestedForkProhibited
        } else {
            AgentError::Fork(e)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            ..Default::default()
        }
    }

    #[rstest]
    #[test]
    fn fork_uses_system_prompt_only() {
        let sys = msg("system", "sys-prompt");
        let fork = ConversationFork {
            parent_session_id: "s".into(),
            frozen_knowledge_snapshots: HashMap::new(),
            agent_def: AgentType::KnowledgeAuditor.definition(),
            task_message: msg("user", "task"),
            max_react_loops: 10,
        };
        assert!(fork.validate().is_ok());
        let built = fork.build_messages(&sys);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].content, "sys-prompt");
        assert_eq!(built[1].content, "task");
    }

    #[rstest]
    #[test]
    fn invalid_max_react_loops() {
        let fork = ConversationFork {
            parent_session_id: "s".into(),
            frozen_knowledge_snapshots: HashMap::new(),
            agent_def: AgentType::KnowledgeAuditor.definition(),
            task_message: msg("user", "task"),
            max_react_loops: 0,
        };
        assert_eq!(fork.validate(), Err(ForkError::InvalidMaxReactLoops(0)));

        let fork2 = ConversationFork {
            max_react_loops: 81,
            ..fork
        };
        assert_eq!(fork2.validate(), Err(ForkError::InvalidMaxReactLoops(81)));
    }

    #[rstest]
    #[test]
    fn empty_task_rejected() {
        let fork = ConversationFork {
            parent_session_id: "s".into(),
            frozen_knowledge_snapshots: HashMap::new(),
            agent_def: AgentType::KnowledgeAuditor.definition(),
            task_message: msg("user", "   "),
            max_react_loops: 10,
        };
        assert_eq!(fork.validate(), Err(ForkError::EmptyTask));
    }

    #[rstest]
    #[test]
    fn nested_fork_rejected() {
        let sys = msg("system", "sys");
        let result = ForkedAgentContext::fork(ForkBuildParams {
            parent_system_message: &sys,
            parent_session_id: "s".into(),
            agent_type: AgentType::KnowledgeAuditor,
            task_prompt: "task".into(),
            max_react_loops: 10,
            knowledge_snapshots: HashMap::new(),
            parent_is_forked: true,
            skill_roots: None,
        });
        assert_eq!(result, Err(ForkError::InvalidMaxReactLoops(0)));
    }

    #[test]
    fn validate_fork_request_rejects_bad_loops() {
        assert_eq!(
            validate_fork_request(false, 0),
            Err(ForkError::InvalidMaxReactLoops(0))
        );
        assert_eq!(
            validate_fork_request(false, 81),
            Err(ForkError::InvalidMaxReactLoops(81))
        );
        assert!(validate_fork_request(false, 10).is_ok());
    }
}
