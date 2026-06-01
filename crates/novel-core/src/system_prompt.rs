/// Static + dynamic system prompt assembly.
/// Prompt text loaded from `prompt/system.md` at compile time via `include_str!`.
pub struct StaticPrompt {
    pub body: String,
}

impl Default for StaticPrompt {
    fn default() -> Self {
        Self {
            body: include_str!("../../../prompt/system.md").into(),
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

    pub fn build(&self, dynamic: &DynamicContext) -> String {
        let mut parts = vec![self.static_layer.body.clone()];
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
            parts.push(format!(
                "## Workspace\n当前作品目录: {}\n其他作品目录: {}/../\nAgent 技能目录: {}/../../skills/",
                dynamic.workspace_path,
                dynamic.workspace_path,
                dynamic.workspace_path
            ));
        }
        parts.join("\n\n")
    }

    /// Static-only system prompt (AGENTS + Workspace frozen; other sections empty).
    pub fn build_static_only(&self, dynamic: &DynamicContext) -> String {
        self.build(&DynamicContext {
            agents_md: dynamic.agents_md.clone(),
            knowledge_index: String::new(),
            memory: String::new(),
            progress: String::new(),
            skill_summaries: Vec::new(),
            workspace_path: dynamic.workspace_path.clone(),
        })
    }
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
        let prompt = b.build(&DynamicContext::default());
        assert!(prompt.contains("小说创作 Agent"));
        assert!(prompt.contains("题材可选文件"));
    }

    #[test]
    fn dynamic_sections_appended() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(&DynamicContext {
            knowledge_index: "林若烟 Ch31".into(),
            ..Default::default()
        });
        assert!(prompt.contains("林若烟 Ch31"));
    }

    #[test]
    fn skill_summaries_render_merged_description() {
        let b = SystemPromptBuilder::new();
        let prompt = b.build(&DynamicContext {
            skill_summaries: vec![
                ("xianxia".into(), "仙侠规范".into()),
                (
                    "post-change".into(),
                    "修改后清单 - 代码改动完成后执行".into(),
                ),
            ],
            ..Default::default()
        });
        assert!(prompt.contains("## Skills"));
        assert!(prompt.contains("- xianxia: 仙侠规范"));
        assert!(prompt.contains("- post-change: 修改后清单 - 代码改动完成后执行"));
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
