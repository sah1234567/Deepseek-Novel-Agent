mod catalog;
mod prompt;

use novel_config::AgentConfig;
use serde::{Deserialize, Serialize};

pub use catalog::{fallback_prompt, system_prompt, ForkAgentCatalogEntry, FORK_AGENT_CATALOG};
pub use novel_config::FORKABLE_AGENT_TYPE_NAMES;
pub use prompt::{format_fork_task, load_agent_prompt};
/// All forkable sub-agent catalog rows.
pub fn fork_agent_catalog() -> &'static [ForkAgentCatalogEntry] {
    FORK_AGENT_CATALOG
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    /// 细纲完成后：计划结构质量审计（大纲对齐、伏笔密度、因果闭合、人物轮换）
    PlanAuditor,
    /// 正文完成后：执行忠实度审计（正文是否忠实执行细纲）
    KnowledgeAuditor,
    ChapterCraftAnalyzer,
    GeneralPurpose,
    /// 后台 extractMemories（非 ForkSubAgent 可选类型）
    MemoryExtractor,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    pub agent_type: AgentType,
    pub name: String,
    pub when_to_use: String,
    pub system_prompt: String,
    pub max_react_loops: u32,
    pub tools: Vec<String>,
}

impl AgentType {
    pub fn definition(self) -> AgentDefinition {
        if self == AgentType::MemoryExtractor {
            return catalog::memory_extractor_definition();
        }
        catalog::catalog_entry(self).to_definition()
    }

    pub fn max_react_loops(self) -> u32 {
        self.definition().max_react_loops
    }

    /// Max ReAct loops from settings when configured; fallback to catalog default.
    pub fn max_react_loops_for(self, cfg: &AgentConfig) -> u32 {
        match self {
            AgentType::KnowledgeAuditor => cfg.knowledge_auditor_max_react_loops,
            other => other.max_react_loops(),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        FORK_AGENT_CATALOG
            .iter()
            .find(|e| e.display_name == name || e.slug == name)
            .map(|e| e.agent_type)
    }

    /// All forkable sub-agent types (catalog order).
    pub fn forkable_types() -> impl Iterator<Item = AgentType> {
        FORK_AGENT_CATALOG.iter().map(|e| e.agent_type)
    }

    /// Union of tool names on any forkable sub-agent allowlist.
    pub fn union_fork_tool_names() -> Vec<String> {
        use std::collections::HashSet;
        let mut names = HashSet::new();
        for entry in FORK_AGENT_CATALOG {
            for t in entry.suggested_tools {
                names.insert((*t).into());
            }
        }
        let mut v: Vec<_> = names.into_iter().collect();
        v.sort();
        v
    }
}

/// Merge settings `always_allow` with every fork sub-agent tool (no Normal-mode Ask for either).
pub fn merge_tool_always_allow(settings: &[String]) -> Vec<String> {
    use std::collections::HashSet;
    let mut names: HashSet<String> = settings.iter().cloned().collect();
    for t in AgentType::union_fork_tool_names() {
        names.insert(t);
    }
    let mut v: Vec<_> = names.into_iter().collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_config::AgentConfig;

    #[test]
    fn knowledge_auditor_has_tracking_query_not_edit() {
        let tools = AgentType::KnowledgeAuditor.definition().tools;
        assert!(tools.contains(&"TrackingQuery".into()));
        assert!(!tools.contains(&"Edit".into()));
        assert!(!tools.contains(&"Write".into()));
    }

    #[test]
    fn chapter_craft_analyzer_is_read_only() {
        let tools = AgentType::ChapterCraftAnalyzer.definition().tools;
        assert!(tools.contains(&"Grep".into()));
        assert!(tools.contains(&"Stats".into()));
        assert!(!tools.contains(&"Edit".into()));
    }

    #[test]
    fn max_react_loops_from_settings() {
        let cfg = AgentConfig {
            knowledge_auditor_max_react_loops: 42,
            ..AgentConfig::default()
        };
        assert_eq!(AgentType::KnowledgeAuditor.max_react_loops_for(&cfg), 42);
    }

    #[test]
    fn general_purpose_definition_includes_write_but_subagent_gated() {
        let tools = AgentType::GeneralPurpose.definition().tools;
        assert!(!tools.contains(&"ForkSubAgent".into()));
        assert!(tools.contains(&"Write".into()));
        assert!(tools.contains(&"WebSearch".into()));
    }

    #[test]
    fn forkable_names_include_plan_auditor() {
        assert!(FORKABLE_AGENT_TYPE_NAMES.contains(&"PlanAuditor"));
        assert!(AgentType::parse("PlanAuditor").is_some());
        assert!(AgentType::parse("plan-auditor").is_some());
    }

    #[test]
    fn plan_auditor_is_read_only() {
        let tools = AgentType::PlanAuditor.definition().tools;
        assert!(tools.contains(&"PlotGraph".into()));
        assert!(tools.contains(&"ForeshadowTracker".into()));
        assert!(tools.contains(&"Corkboard".into()));
        assert!(!tools.contains(&"Edit".into()));
        assert!(!tools.contains(&"Write".into()));
    }

    #[test]
    fn forkable_names_include_existing_agents() {
        assert!(FORKABLE_AGENT_TYPE_NAMES.contains(&"KnowledgeAuditor"));
        assert!(FORKABLE_AGENT_TYPE_NAMES.contains(&"GeneralPurpose"));
        assert!(AgentType::parse("KnowledgeAuditor").is_some());
        assert!(AgentType::parse("ChapterCraftAnalyzer").is_some());
    }

    #[test]
    fn removed_agent_types_not_parseable() {
        assert!(AgentType::parse("LogIntegrityChecker").is_none());
        assert!(AgentType::parse("ConsistencyChecker").is_none());
        assert!(AgentType::parse("DialogueAnalyzer").is_none());
        assert!(AgentType::parse("ChapterWriter").is_none());
    }

    #[test]
    fn union_fork_tool_names_includes_all_fork_agents() {
        let names = AgentType::union_fork_tool_names();
        assert!(names.contains(&"WebSearch".into()));
        assert!(names.contains(&"TrackingQuery".into()));
        assert!(names.contains(&"Write".into()));
    }

    #[test]
    fn merge_tool_always_allow_adds_fork_tools() {
        let merged = super::merge_tool_always_allow(&["Custom".into()]);
        assert!(merged.contains(&"Custom".into()));
        assert!(merged.contains(&"WebSearch".into()));
    }
}
