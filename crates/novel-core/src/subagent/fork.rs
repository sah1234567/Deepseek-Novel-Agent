use crate::context::format_fork_task;
use crate::{AgentDefinition, AgentType, ChatMessage};
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
    pub fn fork(
        parent_system_message: &ChatMessage,
        parent_session_id: String,
        agent_type: AgentType,
        task_prompt: String,
        max_react_loops: u32,
        knowledge_snapshots: HashMap<PathBuf, String>,
        parent_is_forked: bool,
    ) -> Result<Self, ForkError> {
        if parent_is_forked {
            return Err(ForkError::InvalidMaxReactLoops(0)); // mapped to NestedFork in engine
        }
        let agent_def = agent_type.definition();
        if max_react_loops == 0 || max_react_loops > 80 {
            return Err(ForkError::InvalidMaxReactLoops(max_react_loops));
        }
        let formatted_task = format_fork_task(agent_type, &task_prompt, &agent_def.tools)
            .map_err(|_| ForkError::EmptyTask)?;
        let fork = ConversationFork {
            parent_session_id,
            frozen_knowledge_snapshots: knowledge_snapshots,
            agent_def: agent_def.clone(),
            task_message: ChatMessage {
                role: "user".into(),
                content: formatted_task,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            max_react_loops,
        };
        fork.validate()?;
        let messages = fork.build_messages(parent_system_message);
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
        let result = ForkedAgentContext::fork(
            &sys,
            "s".into(),
            AgentType::KnowledgeAuditor,
            "task".into(),
            10,
            HashMap::new(),
            true,
        );
        assert_eq!(result, Err(ForkError::InvalidMaxReactLoops(0)));
    }
}
