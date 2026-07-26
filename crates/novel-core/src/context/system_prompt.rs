//! System prompt assembly.

use crate::interaction_mode::InteractionMode;

/// Static + dynamic system prompt assembly.
///
/// Prompt layers (in order):
/// - `shared_base` — tool conventions, permissions, memory, prohibitions (all agents)
/// - role layer — `orchestrator` (编排) **or** `node_execution` (互动/节点内)
///
/// Domain knowledge (writing SOPs, autonomous strategy) is loaded via InvokeSkill at runtime
/// (`skills/…`), not compiled into the static prompt.
pub struct StaticPrompt {
    pub shared_base: String,
    pub orchestrator: String,
    pub node_execution: String,
}

impl Default for StaticPrompt {
    fn default() -> Self {
        Self {
            shared_base: include_str!("../../../../prompt/shared-base.md").into(),
            orchestrator: include_str!("../../../../prompt/orchestrator.md").into(),
            node_execution: include_str!("../../../../prompt/node-execution.md").into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DynamicContext {
    pub agents_md: String,
    pub knowledge_index: String,
    pub memory: String,
    pub progress: String,
    pub skill_summaries: Vec<(String, String)>,
    /// Absolute canonical path of the current project root (e.g. g:\...\works\default\)
    pub workspace_path: String,
}

pub struct SystemPromptBuilder {
    static_layer: StaticPrompt,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            static_layer: StaticPrompt::default(),
        }
    }

    /// Build the main-agent system prompt.
    /// Layers: shared_base → role (orchestrator | node_execution) → dynamic sections.
    pub fn build(
        &self,
        dynamic: &DynamicContext,
        is_unattended: bool,
        interaction: InteractionMode,
    ) -> String {
        let role = match interaction {
            InteractionMode::Orchestrate => self.static_layer.orchestrator.clone(),
            InteractionMode::Work => self.static_layer.node_execution.clone(),
        };
        let mut parts = vec![self.static_layer.shared_base.clone(), role];
        if is_unattended {
            parts.push(
                "## Mode: Unattended\n请 InvokeSkill('autonomous-writing') 加载自主写作策略。"
                    .into(),
            );
        }
        self.append_dynamic(&mut parts, dynamic);
        parts.join("\n\n")
    }

    /// Build a sub-agent system prompt (minimal: shared_base only, plus optional skill body).
    /// Sub-agents do NOT receive orchestrator or node-domain prompts.
    pub fn build_subagent(&self, skill_body: Option<&str>) -> String {
        let mut parts = vec![self.static_layer.shared_base.clone()];
        if let Some(body) = skill_body {
            parts.push(body.to_string());
        }
        parts.join("\n\n")
    }

    fn append_dynamic(&self, parts: &mut Vec<String>, dynamic: &DynamicContext) {
        if !dynamic.agents_md.is_empty() {
            parts.push(format!("## AGENTS.md\n{}", dynamic.agents_md));
        }
        if !dynamic.knowledge_index.is_empty() {
            parts.push(format!("## Knowledge Index\n{}", dynamic.knowledge_index));
        }
        if !dynamic.memory.is_empty() {
            parts.push(format!("## Memory\n{}", dynamic.memory));
        }
        if !dynamic.progress.is_empty() {
            parts.push(format!("## Progress\n{}", dynamic.progress));
        }
        if !dynamic.skill_summaries.is_empty() {
            let skills: Vec<String> = dynamic
                .skill_summaries
                .iter()
                .map(|(n, d)| format!("- {n}: {d}"))
                .collect();
            parts.push(format!("## Skills\n{}", skills.join("\n")));
        }
        if !dynamic.workspace_path.is_empty() {
            parts.push(workspace_section(&dynamic.workspace_path));
        }
    }

    /// Static-only system prompt (AGENTS + Workspace frozen; other sections empty).
    /// Uses orchestrator role — hash is computed at session birth (default Orchestrate).
    pub fn build_static_only(&self, dynamic: &DynamicContext) -> String {
        let mut parts = vec![
            self.static_layer.shared_base.clone(),
            self.static_layer.orchestrator.clone(),
        ];
        if !dynamic.agents_md.is_empty() {
            parts.push(format!("## AGENTS.md\n{}", dynamic.agents_md));
        }
        if !dynamic.workspace_path.is_empty() {
            parts.push(workspace_section(&dynamic.workspace_path));
        }
        parts.join("\n\n")
    }
}

fn workspace_section(path: &str) -> String {
    format!(
        "## Workspace\n当前作品目录: {path}\n其他作品目录: {path}/../\nAgent 技能目录: {path}/../../skills/"
    )
}

/// Hash of the static system segment for metadata validation.
pub fn system_static_sha256(dynamic: &DynamicContext) -> String {
    use std::hash::{Hash, Hasher};
    let static_prompt = SystemPromptBuilder::new().build_static_only(dynamic);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    static_prompt.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_prompt_loaded() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(
            &DynamicContext::default(),
            false,
            InteractionMode::Orchestrate,
        );
        assert!(prompt.contains("共享底座"), "expected shared-base");
        assert!(prompt.contains("图编排器"), "expected orchestrator");
        // Domain knowledge (system.md) is no longer embedded — loaded via InvokeSkill
        assert!(
            !prompt.contains("小说创作 Agent"),
            "node-domain must NOT be embedded in orchestrator prompt"
        );
    }

    #[test]
    fn dynamic_sections_appended() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(
            &DynamicContext {
                knowledge_index: "林若烟 Ch31".into(),
                ..Default::default()
            },
            false,
            InteractionMode::Orchestrate,
        );
        assert!(prompt.contains("林若烟 Ch31"));
    }

    #[test]
    fn work_mode_uses_node_execution_role() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(&DynamicContext::default(), false, InteractionMode::Work);
        assert!(prompt.contains("节点执行") || prompt.contains("互动（Work）"));
        assert!(!prompt.contains("图编排器"));
    }

    #[test]
    fn skill_summaries_render_merged_description() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(
            &DynamicContext {
                skill_summaries: vec![
                    ("xianxia".into(), "仙侠规范".into()),
                    (
                        "post-change".into(),
                        "修改后清单 - 代码改动完成后执行".into(),
                    ),
                ],
                ..Default::default()
            },
            false,
            InteractionMode::Orchestrate,
        );
        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("- xianxia: 仙侠规范"));
    }

    #[test]
    fn autonomous_prompt_injected_when_unattended() {
        let b = SystemPromptBuilder::new();
        let unattended = b.build(
            &DynamicContext::default(),
            true,
            InteractionMode::Orchestrate,
        );
        assert!(unattended.contains("Mode: Unattended"));
        assert!(unattended.contains("InvokeSkill('autonomous-writing')"));

        let normal = b.build(
            &DynamicContext::default(),
            false,
            InteractionMode::Orchestrate,
        );
        assert!(!normal.contains("Mode: Unattended"));
    }

    #[test]
    fn subagent_prompt_excludes_orchestrator_and_node_domain() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build_subagent(Some("audit instructions here"));
        assert!(
            prompt.contains("共享底座"),
            "sub-agent must have shared-base"
        );
        assert!(
            prompt.contains("audit instructions here"),
            "sub-agent must have skill body"
        );
        assert!(
            !prompt.contains("图编排器"),
            "sub-agent must NOT have orchestrator"
        );
        assert!(
            !prompt.contains("小说创作 Agent"),
            "sub-agent must NOT have node-domain"
        );
    }

    #[test]
    fn subagent_prompt_without_skill_body() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build_subagent(None);
        assert!(prompt.contains("共享底座"));
        assert!(!prompt.contains("图编排器"));
        assert!(!prompt.contains("小说创作 Agent"));
    }

    #[test]
    fn system_static_sha256_ignores_skill_summaries() {
        let base = DynamicContext {
            agents_md: "agents".into(),
            workspace_path: "g:\\works\\demo".into(),
            ..Default::default()
        };
        let mut with_skills = base.clone();
        with_skills.skill_summaries = vec![("xianxia".into(), "仙侠".into())];
        assert_eq!(
            system_static_sha256(&base),
            system_static_sha256(&with_skills)
        );
    }
}
