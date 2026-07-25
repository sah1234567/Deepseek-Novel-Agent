//! Fork task_message assembly (skill bodies / thin shells + runtime constraints).

use super::catalog::{audit_skill_id, fallback_prompt, system_prompt};
use super::AgentType;
use crate::AgentError;
use std::path::Path;

/// Roots used to resolve `skills/{id}/SKILL.md` for audit fork agents.
#[derive(Debug, Clone, Copy)]
pub struct SkillLoadRoots<'a> {
    pub project_root: &'a Path,
    pub agent_skills_dir: Option<&'a Path>,
}

/// Load agent instructions for the fork task prefix.
///
/// Audit agents (`PlanAuditor` / `KnowledgeAuditor` / `ChapterCraftAnalyzer`) load
/// `skills/audit-*/SKILL.md` at runtime. GeneralPurpose uses the thin built-in shell.
/// Missing skill files fall back to [`fallback_prompt`].
pub fn load_agent_prompt(
    agent_type: AgentType,
    roots: Option<SkillLoadRoots<'_>>,
) -> Result<String, AgentError> {
    if let Some(skill_id) = audit_skill_id(agent_type) {
        if let Some(r) = roots {
            if let Some(path) =
                novel_skills::resolve_skill_md(r.project_root, r.agent_skills_dir, skill_id)
            {
                match novel_skills::load_skill(&path) {
                    Ok(skill) if !skill.body.trim().is_empty() => return Ok(skill.body),
                    Ok(_) => {
                        tracing::warn!(
                            skill_id,
                            path = %path.display(),
                            "audit skill body empty; using fallback_prompt"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            skill_id,
                            path = %path.display(),
                            error = %e,
                            "audit skill load failed; using fallback_prompt"
                        );
                    }
                }
            } else {
                tracing::warn!(
                    skill_id,
                    "audit skill not found under project/agent skills; using fallback_prompt"
                );
            }
        }
        return Ok(fallback_prompt(agent_type).to_string());
    }

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
    roots: Option<SkillLoadRoots<'_>>,
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
        let shell = load_agent_prompt(agent_type, roots)?;
        return Ok(format!(
            "{shell}\n\n{runtime_constraints}\n\n---\n\n## 自定义任务\n\n{task}"
        ));
    }

    if agent_type == AgentType::MemoryExtractor {
        // Memory extractor's full task prompt is self-contained in
        // prompt/memory/extraction-task.md — no separate agent shell needed.
        return Ok(task.to_string());
    }

    let agent_prompt = load_agent_prompt(agent_type, roots)?;
    Ok(format!(
        "{agent_prompt}\n\n{runtime_constraints}\n\n---\n\n{task}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::FORK_AGENT_CATALOG;
    use std::path::PathBuf;

    fn repo_skills_roots() -> (PathBuf, PathBuf) {
        // crates/novel-core/src/agent -> repo root
        let agent_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = agent_dir.join("../..").canonicalize().expect("repo root");
        let skills = root.join("skills");
        (root, skills)
    }

    fn audit_roots() -> SkillLoadRoots<'static> {
        // Leak paths for 'static test convenience — process-local only.
        let (root, skills) = repo_skills_roots();
        let root = Box::leak(root.into_boxed_path());
        let skills = Box::leak(skills.into_boxed_path());
        SkillLoadRoots {
            project_root: root,
            agent_skills_dir: Some(skills),
        }
    }

    #[test]
    fn format_fork_task_includes_separator_and_tools() {
        let tools = AgentType::KnowledgeAuditor.definition().tools;
        let t = format_fork_task(
            AgentType::KnowledgeAuditor,
            "审计第1章",
            &tools,
            Some(audit_roots()),
        )
        .expect("task");
        assert!(t.contains("---"));
        assert!(t.contains("审计第1章"));
        assert!(t.contains("禁止嵌套 fork"));
        assert!(t.contains("TrackingQuery"));
        assert!(t.contains("写入门控"));
        assert!(t.contains("场景忠实度") || t.contains("audit-knowledge"));
    }

    #[test]
    fn knowledge_auditor_prompt_loads_from_skill() {
        let p =
            load_agent_prompt(AgentType::KnowledgeAuditor, Some(audit_roots())).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("场景忠实度") || p.contains("忠实度"));
    }

    #[test]
    fn chapter_craft_analyzer_forbids_json() {
        let p = load_agent_prompt(AgentType::ChapterCraftAnalyzer, Some(audit_roots()))
            .expect("prompt");
        assert!(p.contains("禁止 JSON"));
        assert!(p.contains("禁止 fork"));
    }

    #[test]
    fn general_purpose_forbids_report_files() {
        let p = load_agent_prompt(AgentType::GeneralPurpose, None).expect("prompt");
        assert!(p.contains("严禁"));
        assert!(p.contains("assistant 消息正文中返回"));
        assert!(p.contains("门控"));
    }

    #[test]
    fn plan_auditor_prompt_loads_from_skill() {
        let p = load_agent_prompt(AgentType::PlanAuditor, Some(audit_roots())).expect("prompt");
        assert!(p.contains("接下来"));
        assert!(p.contains("大纲对齐"));
    }

    #[test]
    fn plan_auditor_fork_task_has_runtime_constraints() {
        let tools = AgentType::PlanAuditor.definition().tools;
        let t = format_fork_task(
            AgentType::PlanAuditor,
            "审计细纲 ch5",
            &tools,
            Some(audit_roots()),
        )
        .expect("task");
        assert!(t.contains("禁止嵌套 fork"));
        assert!(t.contains("Corkboard"));
    }

    #[test]
    fn general_purpose_fork_task_uses_custom_task_as_body() {
        let tools = AgentType::GeneralPurpose.definition().tools;
        let custom = "对比 chapter-003 与 chapter-005 细纲人物出场";
        let t = format_fork_task(AgentType::GeneralPurpose, custom, &tools, None).expect("task");
        assert!(t.contains("## 自定义任务"));
        assert!(t.contains(custom));
        assert!(t.contains("写入门控"));
    }

    #[test]
    fn memory_extractor_fork_task_is_pass_through() {
        let tools = AgentType::MemoryExtractor.definition().tools;
        let task = "分析最近 5 条消息并更新 memory。";
        let t = format_fork_task(AgentType::MemoryExtractor, task, &tools, None).expect("task");
        assert_eq!(t, task);
    }

    #[test]
    fn missing_skill_falls_back() {
        let tmp = tempfile::TempDir::new().unwrap();
        let roots = SkillLoadRoots {
            project_root: tmp.path(),
            agent_skills_dir: None,
        };
        let p = load_agent_prompt(AgentType::PlanAuditor, Some(roots)).expect("fallback");
        assert!(p.contains("细纲计划审计") || p.contains("大纲对齐"));
    }

    #[test]
    fn fallback_prompt_covers_all_catalog_entries() {
        for entry in FORK_AGENT_CATALOG {
            assert!(!entry.fallback_prompt.is_empty());
        }
    }

    #[test]
    fn audit_skill_ids_mapped() {
        assert_eq!(
            crate::agent::audit_skill_id(AgentType::PlanAuditor),
            Some("audit-plan")
        );
        assert_eq!(
            crate::agent::audit_skill_id(AgentType::KnowledgeAuditor),
            Some("audit-knowledge")
        );
        assert_eq!(
            crate::agent::audit_skill_id(AgentType::ChapterCraftAnalyzer),
            Some("audit-craft")
        );
        assert_eq!(
            crate::agent::audit_skill_id(AgentType::GeneralPurpose),
            None
        );
    }
}
