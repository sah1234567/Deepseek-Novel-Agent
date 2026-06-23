//! Per-job subagent ReAct runner (`run_subagent_job`).

#![allow(clippy::too_many_arguments)]

use crate::engine::session_llm::{build_chat_client, SessionLlmSnapshot};
use crate::hooks::main_tool_schemas;
use crate::message::{assistant_from_completion, to_llm_messages_traced, RepairTraceContext};
use crate::subagent::helpers::{
    execute_subagent_tool_batch, fork_child_push, subagent_push_tool_results,
};
use crate::subagent::llm_turn::{
    fetch_subagent_llm_completion, subagent_after_completion, SubagentLlmFetch,
};
use crate::subagent::react::{
    react_limit_reminder_message, report_only_tool_rejection, SubagentLoopPhase,
};
use crate::subagent::{build_fork_child, SubagentJob, SubagentJobKind};
use crate::turn::TurnContext;
use crate::{AgentError, AgentType, Event, TerminalReason};
use novel_knowledge::truncate_chars;
use tokio::sync::mpsc;

/// Run one subagent job (tool fork or PostToolUse hook). Transcript -> `fork_messages` only.
pub async fn run_subagent_job(
    shared: crate::EngineShared,
    job: SubagentJob,
    fork_run_id: String,
    llm_snap: SessionLlmSnapshot,
    event_tx: Option<mpsc::UnboundedSender<Event>>,
) -> Result<String, AgentError> {
    run_subagent_job_with_child(shared, job, fork_run_id, llm_snap, event_tx, None).await
}

/// Like [`run_subagent_job`] but allows a pre-built child (e.g. memory extraction + recent messages).
pub(crate) async fn run_subagent_job_with_child(
    shared: crate::EngineShared,
    job: SubagentJob,
    fork_run_id: String,
    llm_snap: SessionLlmSnapshot,
    event_tx: Option<mpsc::UnboundedSender<Event>>,
    prebuilt_child: Option<crate::ForkedAgentContext>,
) -> Result<String, AgentError> {
    let agent_type = job.agent_type;
    let task = job.task;
    let silent = matches!(job.kind, SubagentJobKind::MemoryExtraction);
    let parent_tool_call_id = match job.kind {
        SubagentJobKind::ToolFork {
            parent_tool_call_id,
        } => Some(parent_tool_call_id),
        SubagentJobKind::HookAuditor | SubagentJobKind::MemoryExtraction => None,
    };
    tracing::debug!(
        session_id = %shared.session.id,
        ?agent_type,
        task_len = task.len(),
        silent,
        "subagent_job_start"
    );
    let mut llm = build_chat_client(&llm_snap, &shared.global_config_path);
    let mut child = match prebuilt_child {
        Some(c) => c,
        None => build_fork_child(&shared, agent_type, task.clone())?,
    };

    if !silent {
        let task_preview = truncate_chars(&task, 80);
        if let Some(tx) = event_tx.as_ref() {
            let _ = tx.send(Event::SubAgentStarted {
                fork_run_id: fork_run_id.clone(),
                agent_id: agent_type.to_string(),
                agent_type: agent_type.to_string(),
                task_preview,
                parent_tool_call_id: parent_tool_call_id.clone(),
            });
        }
        crate::subagent::fork_transcript::persist_fork_message(
            &shared.session.db,
            &fork_run_id,
            &child.fork.task_message,
        )?;
    }

    let schemas = main_tool_schemas(&shared.registry);
    let max_react_loops = child.fork.max_react_loops;

    subagent_run_react_loop(
        &shared,
        &mut llm,
        &llm_snap,
        agent_type,
        &task,
        &fork_run_id,
        &schemas,
        &mut child,
        max_react_loops,
        if silent { None } else { event_tx.as_ref() },
    )
    .await
}

fn subagent_finalize_output(
    child: &crate::ForkedAgentContext,
    agent_type: AgentType,
    was_interrupted: bool,
    interrupt_reason: &str,
    inner_turn: u32,
    max_react_loops: u32,
) -> String {
    let last_assistant = child
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "（无文本输出）".into());
    if !was_interrupted {
        return last_assistant;
    }
    let task_preview = truncate_chars(&child.fork.task_message.content, 200);
    format!(
        "[子 Agent 已中断: {}]\n\n\
         ⚠ 用户中断。部分修改可能已写入磁盘，KB 可能处于不一致状态。\n\n\
         - 任务: {}\n\
         - 已执行轮数: {} / {} 轮\n\
         - 中断原因: {}\n\n\
         ## 最后输出\n\n{}\n\n\
         ## 说明\n\
         请根据最后输出和已执行轮数判断后续操作。如有文件写入，建议验证一致性后再继续。",
        agent_type, task_preview, inner_turn, max_react_loops, interrupt_reason, last_assistant,
    )
}

enum SubagentTurnOutcome {
    Break {
        interrupted: bool,
        reason: &'static str,
    },
    ReturnReport(String),
    Continue {
        phase: SubagentLoopPhase,
    },
}

async fn subagent_single_turn(
    shared: &crate::EngineShared,
    llm: &mut Option<novel_deepseek::ChatClient>,
    llm_snap: &SessionLlmSnapshot,
    agent_type: AgentType,
    task: &str,
    fork_run_id: &str,
    schemas: &[(String, String, serde_json::Value)],
    child: &mut crate::ForkedAgentContext,
    turn_ctx: &mut TurnContext,
    phase: SubagentLoopPhase,
    max_react_loops: u32,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<SubagentTurnOutcome, AgentError> {
    let llm_msgs = to_llm_messages_traced(
        &child.messages,
        Some(RepairTraceContext {
            label: "subagent_job",
            fork_run_id: Some(fork_run_id),
            inner_turn: Some(turn_ctx.inner_turn),
            session_id: Some(&shared.session.id),
        }),
    );
    let snapshot = child.messages.clone();
    let active_schemas: &[(String, String, serde_json::Value)] =
        if phase.is_report_only() { &[] } else { schemas };

    let fetch = fetch_subagent_llm_completion(
        llm,
        shared,
        agent_type,
        task,
        fork_run_id,
        &llm_msgs,
        active_schemas,
        llm_snap,
        event_tx,
        &mut child.messages,
        &snapshot,
        turn_ctx.inner_turn,
    )
    .await?;
    let (completion, fork_dispatch) = match fetch {
        SubagentLlmFetch::LoopBreak => {
            return Ok(SubagentTurnOutcome::Break {
                interrupted: true,
                reason: "流中断或 API error",
            });
        }
        SubagentLlmFetch::ReturnReport(report) => {
            return Ok(SubagentTurnOutcome::ReturnReport(report))
        }
        SubagentLlmFetch::Completion {
            completion,
            fork_dispatch,
        } => (completion, fork_dispatch),
    };

    if let Some(report) = subagent_after_completion(
        shared,
        agent_type,
        task,
        fork_run_id,
        &mut child.messages,
        &completion,
        llm_snap,
        event_tx,
        turn_ctx.inner_turn,
    )? {
        return Ok(SubagentTurnOutcome::ReturnReport(report));
    }

    subagent_apply_completion_tools(
        shared,
        agent_type,
        fork_run_id,
        child,
        turn_ctx,
        phase,
        max_react_loops,
        &completion,
        fork_dispatch,
        event_tx,
    )
    .await
}

async fn subagent_apply_completion_tools(
    shared: &crate::EngineShared,
    agent_type: AgentType,
    fork_run_id: &str,
    child: &mut crate::ForkedAgentContext,
    turn_ctx: &mut TurnContext,
    phase: SubagentLoopPhase,
    max_react_loops: u32,
    completion: &novel_deepseek::LlmCompletion,
    fork_dispatch: Option<std::sync::Arc<std::sync::Mutex<crate::turn::StreamingToolDispatch>>>,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<SubagentTurnOutcome, AgentError> {
    let tool_calls = completion.tool_calls.clone();
    if tool_calls.is_empty() {
        fork_child_push(
            &shared.session.db,
            fork_run_id,
            &mut child.messages,
            assistant_from_completion(completion),
        )?;
        return Ok(SubagentTurnOutcome::Break {
            interrupted: false,
            reason: "",
        });
    }

    if phase.is_report_only() {
        fork_child_push(
            &shared.session.db,
            fork_run_id,
            &mut child.messages,
            assistant_from_completion(completion),
        )?;
        for tc in &tool_calls {
            fork_child_push(
                &shared.session.db,
                fork_run_id,
                &mut child.messages,
                report_only_tool_rejection(&tc.id),
            )?;
        }
        if let Some(next) = phase.consume_grace() {
            return Ok(SubagentTurnOutcome::Continue { phase: next });
        }
        return Ok(SubagentTurnOutcome::Break {
            interrupted: true,
            reason: "报告收尾失败",
        });
    }

    let fork_ctx = crate::subagent::helpers::subagent_tool_context(shared, agent_type);
    let results = execute_subagent_tool_batch(
        &shared.registry,
        &fork_ctx,
        fork_dispatch,
        &tool_calls,
        event_tx,
        fork_run_id,
        &shared.fork_stream_subs,
    )
    .await?;
    subagent_push_tool_results(
        &shared.registry,
        &shared.session.db,
        fork_run_id,
        &mut child.messages,
        agent_type,
        turn_ctx,
        &tool_calls,
        completion,
        results,
        event_tx,
        &shared.fork_stream_subs,
    )?;

    if let Err(TerminalReason::MaxReactLoops(_)) = turn_ctx.increment_inner() {
        let spent = turn_ctx.inner_spent();
        fork_child_push(
            &shared.session.db,
            fork_run_id,
            &mut child.messages,
            react_limit_reminder_message(spent, max_react_loops),
        )?;
        return Ok(SubagentTurnOutcome::Continue {
            phase: phase.enter_report_only(),
        });
    }
    Ok(SubagentTurnOutcome::Continue { phase })
}

async fn subagent_run_react_loop(
    shared: &crate::EngineShared,
    llm: &mut Option<novel_deepseek::ChatClient>,
    llm_snap: &SessionLlmSnapshot,
    agent_type: AgentType,
    task: &str,
    fork_run_id: &str,
    schemas: &[(String, String, serde_json::Value)],
    child: &mut crate::ForkedAgentContext,
    max_react_loops: u32,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<String, AgentError> {
    let mut turn_ctx = TurnContext::new(max_react_loops);
    let mut phase = SubagentLoopPhase::Reacting;
    let was_interrupted;
    let interrupt_reason;
    loop {
        if shared.abort_controller.is_aborted() {
            was_interrupted = true;
            interrupt_reason = "用户取消";
            break;
        }
        if matches!(phase, SubagentLoopPhase::Reacting) && !turn_ctx.needs_continuation() {
            let spent = turn_ctx.inner_spent();
            let reminder = react_limit_reminder_message(spent, max_react_loops);
            fork_child_push(
                &shared.session.db,
                fork_run_id,
                &mut child.messages,
                reminder,
            )?;
            phase = phase.enter_report_only();
        }
        tracing::debug!(
            session_id = %shared.session.id,
            %fork_run_id,
            ?agent_type,
            inner_turn = turn_ctx.inner_turn,
            message_count = child.messages.len(),
            phase = %if phase.is_report_only() { "report_only" } else { "reacting" },
            "subagent_inner_turn_start"
        );
        match subagent_single_turn(
            shared,
            llm,
            llm_snap,
            agent_type,
            task,
            fork_run_id,
            schemas,
            child,
            &mut turn_ctx,
            phase,
            max_react_loops,
            event_tx,
        )
        .await?
        {
            SubagentTurnOutcome::Break {
                interrupted,
                reason,
            } => {
                was_interrupted = interrupted;
                interrupt_reason = reason;
                break;
            }
            SubagentTurnOutcome::ReturnReport(report) => return Ok(report),
            SubagentTurnOutcome::Continue { phase: next } => {
                phase = next;
                continue;
            }
        }
    }

    let output = subagent_finalize_output(
        child,
        agent_type,
        was_interrupted,
        interrupt_reason,
        turn_ctx.inner_turn,
        max_react_loops,
    );

    // Decrement sub-agent count (paired with `sub_agent_inc` at spawn site).
    shared.sub_agent_dec();

    if agent_type != AgentType::MemoryExtractor {
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::SubAgentComplete {
                fork_run_id: fork_run_id.to_string(),
                agent_id: agent_type.to_string(),
                output: output.clone(),
                cache_hit_rate: 0.0,
            });
        }
    }

    tracing::debug!(
        session_id = %shared.session.id,
        ?agent_type,
        output_len = output.len(),
        was_interrupted,
        inner_turns = turn_ctx.inner_turn,
        "subagent_job_complete"
    );

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::build_fork_child;
    use crate::EngineConfig;
    use novel_deepseek::LlmCompletion;
    use tempfile::TempDir;

    fn fork_setup(tmp: &TempDir) -> (crate::AgentEngine, String, crate::ForkedAgentContext) {
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = crate::AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let fork_run_id = engine
            .shared
            .session
            .db
            .create_fork_run(
                &engine.shared.session.id,
                1,
                "KnowledgeAuditor",
                "t",
                "test",
            )
            .unwrap();
        let child =
            build_fork_child(&engine.shared, AgentType::KnowledgeAuditor, "audit".into()).unwrap();
        (engine, fork_run_id, child)
    }

    #[tokio::test]
    async fn empty_tools_breaks_subagent_turn() {
        let tmp = TempDir::new().unwrap();
        let (engine, fork_run_id, mut child) = fork_setup(&tmp);
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: Some("report only text".into()),
            reasoning_content: None,
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        };
        let out = subagent_apply_completion_tools(
            &engine.shared,
            AgentType::KnowledgeAuditor,
            &fork_run_id,
            &mut child,
            &mut turn_ctx,
            SubagentLoopPhase::Reacting,
            40,
            &completion,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            out,
            SubagentTurnOutcome::Break {
                interrupted: false,
                reason: ""
            }
        ));
    }

    #[tokio::test]
    async fn react_tools_batch_continues() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sub.txt"), "fork body").unwrap();
        let (engine, fork_run_id, mut child) = fork_setup(&tmp);
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: Some("reading".into()),
            reasoning_content: None,
            tool_calls: vec![novel_deepseek::LlmToolCall {
                id: "r1".into(),
                name: "Read".into(),
                arguments: r#"{"file_path":"sub.txt"}"#.into(),
            }],
            stop_reason: Some("tool_calls".into()),
            usage: None,
        };
        let out = subagent_apply_completion_tools(
            &engine.shared,
            AgentType::KnowledgeAuditor,
            &fork_run_id,
            &mut child,
            &mut turn_ctx,
            SubagentLoopPhase::Reacting,
            40,
            &completion,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            out,
            SubagentTurnOutcome::Continue {
                phase: SubagentLoopPhase::Reacting
            }
        ));
        assert!(child.messages.iter().any(|m| m.role == "tool"));
    }

    #[tokio::test]
    async fn react_limit_enters_report_only_phase() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sub.txt"), "fork body").unwrap();
        let (engine, fork_run_id, mut child) = fork_setup(&tmp);
        let mut turn_ctx = TurnContext::new(1);
        let completion = LlmCompletion {
            content: None,
            reasoning_content: None,
            tool_calls: vec![novel_deepseek::LlmToolCall {
                id: "r1".into(),
                name: "Read".into(),
                arguments: r#"{"file_path":"sub.txt"}"#.into(),
            }],
            stop_reason: Some("tool_calls".into()),
            usage: None,
        };
        let out = subagent_apply_completion_tools(
            &engine.shared,
            AgentType::KnowledgeAuditor,
            &fork_run_id,
            &mut child,
            &mut turn_ctx,
            SubagentLoopPhase::Reacting,
            1,
            &completion,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            out,
            SubagentTurnOutcome::Continue {
                phase: SubagentLoopPhase::ReportOnly { grace_left: 1 }
            }
        ));
    }

    #[tokio::test]
    async fn report_only_tools_enters_grace_phase() {
        let tmp = TempDir::new().unwrap();
        let (engine, fork_run_id, mut child) = fork_setup(&tmp);
        let phase = SubagentLoopPhase::ReportOnly { grace_left: 1 };
        let completion = LlmCompletion {
            content: Some("done".into()),
            reasoning_content: None,
            tool_calls: vec![novel_deepseek::LlmToolCall {
                id: "t1".into(),
                name: "Read".into(),
                arguments: "{}".into(),
            }],
            stop_reason: Some("tool_calls".into()),
            usage: None,
        };
        let mut turn_ctx = TurnContext::new(40);
        let out = subagent_apply_completion_tools(
            &engine.shared,
            AgentType::KnowledgeAuditor,
            &fork_run_id,
            &mut child,
            &mut turn_ctx,
            phase,
            40,
            &completion,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            out,
            SubagentTurnOutcome::Continue {
                phase: SubagentLoopPhase::ReportOnly { grace_left: 0 }
            }
        ));
    }
}
