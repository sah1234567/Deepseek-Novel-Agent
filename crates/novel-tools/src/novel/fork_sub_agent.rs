use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput, ValidationError};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ForkSubAgentTool;

const VALID_AGENT_TYPES: &[&str] = &[
    "PlanAuditor",
    "KnowledgeAuditor",
    "ChapterCraftAnalyzer",
    "GeneralPurpose",
];

#[async_trait]
impl Tool for ForkSubAgentTool {
    fn name(&self) -> &str {
        "ForkSubAgent"
    }

    fn description(&self) -> &str {
        "Fork a read-only or custom sub-agent with shared system prompt (KV cache). \
         Foreground tool: the main session waits until all sub-agents from this batch finish, \
         then receives their reports before continuing. Sub-agents cannot fork again."
    }

    fn usage_hint(&self) -> &str {
        "After each 细纲 Write: PlanAuditor. After each chapter Write: KnowledgeAuditor + ChapterCraftAnalyzer (same message, parallel). \
         All subagents in one assistant message run in parallel."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_type": {
                    "type": "string",
                    "enum": VALID_AGENT_TYPES,
                    "description": "Sub-agent type. GeneralPurpose = custom one-off subagent (task is the full prompt)."
                },
                "task": {
                    "type": "string",
                    "description": "Task for predefined agents, or full custom prompt when agent_type is GeneralPurpose"
                },
                "description": {
                    "type": "string",
                    "description": "Optional short label for logs/UI (default: custom subagent)"
                }
            },
            "required": ["agent_type", "task"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn allowed_in_plan_mode(&self) -> bool {
        true
    }

    fn skips_normal_permission_ask(&self) -> bool {
        true
    }

    fn blocks_nested_fork(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        enqueue_fork_subagent(&input, ctx)
    }
}

pub(crate) fn enqueue_fork_subagent(
    input: &Value,
    ctx: &ToolContext,
) -> Result<ToolOutput, ToolError> {
    if !ctx.allow_fork {
        return Err(ToolError::PermissionDenied(
            "子 Agent 禁止嵌套 fork。子 Agent 共用父 system prompt，仅通过 task_message 注入指令。"
                .into(),
        ));
    }
    let agent_type = require_str(input, "agent_type")?;
    if !VALID_AGENT_TYPES.contains(&agent_type.as_str()) {
        return Err(ToolError::Validation(ValidationError::InvalidField(
            format!("agent_type: {agent_type}"),
        )));
    }
    let task_raw = require_str(input, "task")?;
    let task = task_raw.trim();
    if task.is_empty() {
        return Err(ToolError::Validation(ValidationError::MissingField(
            "task".into(),
        )));
    }
    let parent_tool_call_id = ctx
        .current_tool_call_id
        .clone()
        .ok_or_else(|| ToolError::Internal("fork queue missing tool_call_id".into()))?;
    let queue = ctx
        .subagent_queue
        .as_ref()
        .ok_or_else(|| ToolError::Internal("subagent queue not configured on engine".into()))?;
    let mut guard = queue
        .lock()
        .map_err(|_| ToolError::Internal("subagent queue lock poisoned".into()))?;
    let agent_label = agent_type.clone();
    guard.push(crate::PendingSubagentWork {
        agent_type,
        task: task.to_string(),
        parent_tool_call_id: Some(parent_tool_call_id),
    });
    let label = input
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("custom subagent");
    Ok(ToolOutput {
        content: format!(
            "已启动 {agent_label}（{label}）。主会话将等待本批 Subagent 全部完成并注入报告后再继续；子 Agent 不可再 fork。"
        ),
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use std::sync::{Arc, Mutex};

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_fork_when_not_allowed() {
        let tool = ForkSubAgentTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: false,
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        let err = tool
            .call(
                json!({"agent_type": "KnowledgeAuditor", "task": "审计第1章"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("禁止嵌套 fork"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enqueues_fork_request() {
        let tool = ForkSubAgentTool;
        let queue = Arc::new(Mutex::new(Vec::new()));
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: true,
            subagent_queue: Some(Arc::clone(&queue)),
            current_tool_call_id: Some("tc-1".into()),
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        tool.call(
            json!({"agent_type": "KnowledgeAuditor", "task": "扫描第1章"}),
            &ctx,
        )
        .await
        .unwrap();
        let guard = queue.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard[0].agent_type, "KnowledgeAuditor");
        assert_eq!(guard[0].parent_tool_call_id.as_deref(), Some("tc-1"));
    }

    #[test]
    fn rejects_empty_task() {
        let tmp = std::path::PathBuf::from(".");
        let queue = Arc::new(Mutex::new(Vec::new()));
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: true,
            subagent_queue: Some(Arc::clone(&queue)),
            current_tool_call_id: Some("tc-1".into()),
            ..ToolContext::new(tmp)
        };
        let err = enqueue_fork_subagent(
            &json!({"agent_type": "KnowledgeAuditor", "task": "   "}),
            &ctx,
        )
        .unwrap_err();
        assert!(err.to_string().contains("task"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_removed_agent_types() {
        let tool = ForkSubAgentTool;
        let queue = Arc::new(Mutex::new(Vec::new()));
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: true,
            subagent_queue: Some(Arc::clone(&queue)),
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        let err = tool
            .call(
                json!({"agent_type": "ConsistencyChecker", "task": "审计"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("agent_type"));
    }
}
