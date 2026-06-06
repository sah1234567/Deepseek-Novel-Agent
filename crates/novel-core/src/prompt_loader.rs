use crate::{AgentError, AgentType};

fn fallback_prompt(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::PlanAuditor => {
            "你是细纲计划审计 Agent。只读检查大纲对齐、伏笔密度、因果闭合、人物轮换、字数分配、登记完整性，输出自然语言报告与「接下来」指引。"
        }
        AgentType::KnowledgeAuditor => {
            "你是知识库审计 Agent。只读检查正文执行忠实度与收尾完整性，输出自然语言报告与「接下来」指引。"
        }
        AgentType::ChapterCraftAnalyzer => {
            "你是章节文笔分析 Agent。分析对话、节奏、情感、设定一致性，输出自然语言报告。禁止 fork 与 JSON。"
        }
        AgentType::GeneralPurpose => {
            "你是通用子 Agent。严格按下方自定义任务执行，结论写在返回正文中，禁止为说明新建文件。禁止 fork。"
        }
    }
}

fn embedded_prompt(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::PlanAuditor => include_str!("../../../prompt/agents/plan-auditor.md"),
        AgentType::KnowledgeAuditor => include_str!("../../../prompt/agents/knowledge-auditor.md"),
        AgentType::ChapterCraftAnalyzer => {
            include_str!("../../../prompt/agents/chapter-craft-analyzer.md")
        }
        AgentType::GeneralPurpose => include_str!("../../../prompt/agents/general_purpose.md"),
    }
}

/// Load agent-specific instructions injected into fork task_message prefix.
pub fn load_agent_prompt(agent_type: AgentType) -> Result<String, AgentError> {
    let body = embedded_prompt(agent_type).trim();
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
        - **KV cache：** 共用父会话 `messages[0]` system prompt，不可修改 system 层\n\
        - **禁止嵌套 fork：** 无 ForkSubAgent 工具；`sub_agent_running` 时引擎拒绝孙辈 Agent\n\
        - **本轮可用工具（仅此列表）：** {tools_line}\n\
        - 勿调用 system 中列出但不在此列表的工具"
    );

    if agent_type == AgentType::GeneralPurpose {
        let shell = load_agent_prompt(agent_type)?;
        return Ok(format!(
            "{shell}\n\n{runtime_constraints}\n\n---\n\n## 自定义任务\n\n{task}"
        ));
    }

    let agent_prompt = load_agent_prompt(agent_type)?;
    Ok(format!(
        "{agent_prompt}\n\n{runtime_constraints}\n\n---\n\n{task}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_fork_task_includes_separator_and_tools() {
        let tools = AgentType::KnowledgeAuditor.definition().tools;
        let t = format_fork_task(AgentType::KnowledgeAuditor, "审计第1章", &tools).expect("task");
        assert!(t.contains("---"));
        assert!(t.contains("审计第1章"));
        assert!(t.contains("禁止嵌套 fork"));
        assert!(t.contains("TrackingQuery"));
    }

    #[test]
    fn knowledge_auditor_prompt_has_next_steps() {
        let p = load_agent_prompt(AgentType::KnowledgeAuditor).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("读不到本 prompt"));
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
    }

    #[test]
    fn plan_auditor_prompt_has_next_steps() {
        let p = load_agent_prompt(AgentType::PlanAuditor).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("读不到本 prompt"));
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
    }
}
