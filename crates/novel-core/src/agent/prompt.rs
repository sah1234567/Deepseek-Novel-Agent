//! Fork task_message assembly (catalog prompts + runtime constraints).

use super::catalog::{fallback_prompt, system_prompt};
use super::AgentType;
use crate::AgentError;

/// Load agent-specific instructions injected into fork task_message prefix.
pub fn load_agent_prompt(agent_type: AgentType) -> Result<String, AgentError> {
    let body = system_prompt(agent_type).trim();
    if body.is_empty() {
        return Ok(fallback_prompt(agent_type).to_string());
    }
    Ok(body.to_string())
}

/// Format task message: agent instructions + constraints + user task.
pub fn format_fork_task(
    agent_type: AgentType,
    user_task: &str,
    allowed_tools: &[String],
) -> Result<String, AgentError> {
    let task = user_task.trim();
    if task.is_empty() {
        return Err(AgentError::Validation("empty fork task".into()));
    }
    let tools_line = allowed_tools.join(", ");
    let runtime_constraints = format!(
        "## 子 Agent 运行时约束\n\
        - **禁止嵌套 fork：** 无 ForkSubAgent 工具；不得再派出子 Agent\n\
        - **写入门控：** 子 Agent 运行时 Write/Edit/TodoWrite 会被拒绝；勿调用。结论写在最终 assistant 正文\n\
        - **工具定义：** 与主 Agent 相同（缓存对齐）；优先使用下方「建议优先工具」列表中的只读工具\n\
        - **建议优先工具：** {tools_line}"
    );

    if agent_type == AgentType::GeneralPurpose {
        let shell = load_agent_prompt(agent_type)?;
        return Ok(format!(
            "{shell}\n\n{runtime_constraints}\n\n---\n\n## 自定义任务\n\n{task}"
        ));
    }

    if agent_type == AgentType::MemoryExtractor {
        // Memory extractor's full task prompt is self-contained in
        // prompt/memory/extraction-task.md — no separate agent shell needed.
        return Ok(task.to_string());
    }

    let agent_prompt = load_agent_prompt(agent_type)?;
    Ok(format!(
        "{agent_prompt}\n\n{runtime_constraints}\n\n---\n\n{task}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::FORK_AGENT_CATALOG;

    #[test]
    fn format_fork_task_includes_separator_and_tools() {
        let tools = AgentType::KnowledgeAuditor.definition().tools;
        let t = format_fork_task(AgentType::KnowledgeAuditor, "审计第1章", &tools).expect("task");
        assert!(t.contains("---"));
        assert!(t.contains("审计第1章"));
        assert!(t.contains("禁止嵌套 fork"));
        assert!(t.contains("TrackingQuery"));
        assert!(t.contains("写入门控"));
    }

    #[test]
    fn knowledge_auditor_prompt_has_next_steps() {
        let p = load_agent_prompt(AgentType::KnowledgeAuditor).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("子 Agent 完成: KnowledgeAuditor"));
    }

    #[test]
    fn chapter_craft_analyzer_forbids_json() {
        let p = load_agent_prompt(AgentType::ChapterCraftAnalyzer).expect("prompt");
        assert!(p.contains("禁止 JSON"));
        assert!(p.contains("禁止 fork"));
    }

    #[test]
    fn general_purpose_forbids_report_files() {
        let p = load_agent_prompt(AgentType::GeneralPurpose).expect("prompt");
        assert!(p.contains("严禁"));
        assert!(p.contains("assistant 消息正文中返回"));
        assert!(p.contains("门控"));
    }

    #[test]
    fn plan_auditor_prompt_has_next_steps() {
        let p = load_agent_prompt(AgentType::PlanAuditor).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("子 Agent 完成: PlanAuditor"));
        assert!(p.contains("大纲对齐"));
    }

    #[test]
    fn plan_auditor_fork_task_has_runtime_constraints() {
        let tools = AgentType::PlanAuditor.definition().tools;
        let t = format_fork_task(AgentType::PlanAuditor, "审计细纲 ch5", &tools).expect("task");
        assert!(t.contains("禁止嵌套 fork"));
        assert!(t.contains("Corkboard"));
    }

    #[test]
    fn general_purpose_fork_task_uses_custom_task_as_body() {
        let tools = AgentType::GeneralPurpose.definition().tools;
        let custom = "对比 chapter-003 与 chapter-005 细纲人物出场";
        let t = format_fork_task(AgentType::GeneralPurpose, custom, &tools).expect("task");
        assert!(t.contains("## 自定义任务"));
        assert!(t.contains(custom));
        assert!(t.contains("写入门控"));
    }

    #[test]
    fn memory_extractor_fork_task_is_pass_through() {
        // MemoryExtractor's task is self-contained in prompt/memory/extraction-task.md;
        // format_fork_task passes it through unchanged (no agent shell wrapper).
        let tools = AgentType::MemoryExtractor.definition().tools;
        let task = "分析最近 5 条消息并更新 memory。";
        let t = format_fork_task(AgentType::MemoryExtractor, task, &tools).expect("task");
        assert_eq!(t, task);
    }

    #[test]
    fn fallback_prompt_covers_all_catalog_entries() {
        for entry in FORK_AGENT_CATALOG {
            let text = fallback_prompt(entry.agent_type);
            assert_eq!(text, entry.fallback_prompt);
            assert!(!text.is_empty());
            assert!(text.contains("Agent"));
        }
    }
}
