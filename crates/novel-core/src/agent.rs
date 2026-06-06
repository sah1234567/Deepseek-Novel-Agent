use novel_config::AgentConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    /// 细纲完成后：计划结构质量审计（大纲对齐、伏笔密度、因果闭合、人物轮换）
    PlanAuditor,
    /// 正文完成后：执行忠实度审计（正文是否忠实执行细纲）
    KnowledgeAuditor,
    ChapterCraftAnalyzer,
    GeneralPurpose,
}

/// Agent types the main session may fork via `ForkSubAgent`.
pub const FORKABLE_AGENT_TYPE_NAMES: &[&str] = &[
    "PlanAuditor",
    "KnowledgeAuditor",
    "ChapterCraftAnalyzer",
    "GeneralPurpose",
];

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
        match self {
            AgentType::PlanAuditor => AgentDefinition {
                agent_type: self,
                name: "plan-auditor".into(),
                when_to_use: "细纲完成后的计划结构质量审计（大纲对齐、伏笔密度、因果闭合、人物轮换、字数分配）".into(),
                system_prompt: include_str!("../../../prompt/agents/plan-auditor.md").into(),
                max_react_loops: 30,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "PlotGraph".into(),
                    "ForeshadowTracker".into(),
                    "CharacterSearch".into(),
                    "TrackingQuery".into(),
                    "RelationQuery".into(),
                    "Corkboard".into(),
                    "Tail".into(),
                    "Stats".into(),
                ],
            },
            AgentType::KnowledgeAuditor => AgentDefinition {
                agent_type: self,
                name: "knowledge-auditor".into(),
                when_to_use: "知识库遗漏扫描 + 设定一致性深度审计（只读报告）".into(),
                system_prompt: include_str!("../../../prompt/agents/knowledge-auditor.md").into(),
                max_react_loops: 40,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                    "PlotGraph".into(),
                    "Tail".into(),
                    "TrackingQuery".into(),
                    "RelationQuery".into(),
                    "ForeshadowTracker".into(),
                ],
            },
            AgentType::ChapterCraftAnalyzer => AgentDefinition {
                agent_type: self,
                name: "chapter-craft-analyzer".into(),
                when_to_use: "对话质量 + 叙事节奏 + 情感轨迹 + 设定一致性（只读报告）".into(),
                system_prompt: include_str!("../../../prompt/agents/chapter-craft-analyzer.md")
                    .into(),
                max_react_loops: 25,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                    "Stats".into(),
                    "Tail".into(),
                    "TrackingQuery".into(),
                    "RelationQuery".into(),
                ],
            },
            AgentType::GeneralPurpose => AgentDefinition {
                agent_type: self,
                name: "general-purpose".into(),
                when_to_use: "一次性自定义任务：调研、批量整理、特殊分析".into(),
                system_prompt: include_str!("../../../prompt/agents/general_purpose.md").into(),
                max_react_loops: 20,
                tools: vec![
                    "Read".into(),
                    "Write".into(),
                    "Edit".into(),
                    "Grep".into(),
                    "Glob".into(),
                    "CharacterSearch".into(),
                    "PlotGraph".into(),
                    "Tail".into(),
                    "Stats".into(),
                    "InvokeSkill".into(),
                    "ImpactAnalysis".into(),
                    "TodoWrite".into(),
                    "WebSearch".into(),
                ],
            },
        }
    }

    pub fn all_forkable_names() -> &'static [&'static str] {
        FORKABLE_AGENT_TYPE_NAMES
    }

    pub fn is_forkable(name: &str) -> bool {
        Self::parse(name).is_some()
    }

    pub fn max_react_loops(self) -> u32 {
        self.definition().max_react_loops
    }

    /// Max ReAct loops from settings when configured; fallback to `definition().max_react_loops`.
    pub fn max_react_loops_for(self, cfg: &AgentConfig) -> u32 {
        match self {
            AgentType::KnowledgeAuditor => cfg.knowledge_auditor_max_react_loops,
            other => other.max_react_loops(),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "PlanAuditor" | "plan-auditor" => Some(AgentType::PlanAuditor),
            "KnowledgeAuditor" | "knowledge-auditor" => Some(AgentType::KnowledgeAuditor),
            "ChapterCraftAnalyzer" | "chapter-craft-analyzer" => {
                Some(AgentType::ChapterCraftAnalyzer)
            }
            "GeneralPurpose" | "general-purpose" => Some(AgentType::GeneralPurpose),
            _ => None,
        }
    }

    /// Union of tool names on any forkable sub-agent allowlist.
    pub fn union_fork_tool_names() -> Vec<String> {
        use std::collections::HashSet;
        let mut names = HashSet::new();
        for agent in [
            AgentType::PlanAuditor,
            AgentType::KnowledgeAuditor,
            AgentType::ChapterCraftAnalyzer,
            AgentType::GeneralPurpose,
        ] {
            for t in agent.definition().tools {
                names.insert(t);
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
    fn general_purpose_has_write_not_fork() {
        let tools = AgentType::GeneralPurpose.definition().tools;
        assert!(!tools.contains(&"ForkSubAgent".into()));
        assert!(tools.contains(&"Write".into()));
        assert!(tools.contains(&"WebSearch".into()));
    }

    #[test]
    fn forkable_names_include_plan_auditor() {
        assert!(AgentType::all_forkable_names().contains(&"PlanAuditor"));
        assert!(AgentType::is_forkable("PlanAuditor"));
        assert!(AgentType::is_forkable("plan-auditor"));
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
        assert!(AgentType::all_forkable_names().contains(&"KnowledgeAuditor"));
        assert!(AgentType::all_forkable_names().contains(&"GeneralPurpose"));
        assert!(AgentType::is_forkable("KnowledgeAuditor"));
        assert!(AgentType::is_forkable("ChapterCraftAnalyzer"));
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
