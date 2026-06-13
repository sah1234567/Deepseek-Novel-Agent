//! Declarative catalog for forkable sub-agents (SSOT for loops + suggested tools).
//!
//! `AgentType::definition()` is built from [`FORK_AGENT_CATALOG`].
//! LLM API `tools` schemas use the full main registry (`main_tool_schemas`); entries here
//! only drive `format_fork_task` tools_line and documentation.
//!
//! `agent_type` names and default loop limits: `novel_config::fork_agents` (re-exported as
//! [`super::FORKABLE_AGENT_TYPE_NAMES`]).

use super::{AgentDefinition, AgentType};
use novel_config::{
    CHAPTER_CRAFT_ANALYZER_MAX_REACT_LOOPS, GENERAL_PURPOSE_MAX_REACT_LOOPS,
    KNOWLEDGE_AUDITOR_MAX_REACT_LOOPS_DEFAULT, PLAN_AUDITOR_MAX_REACT_LOOPS,
};

/// One forkable sub-agent row in the catalog.
#[derive(Debug, Clone, Copy)]
pub struct ForkAgentCatalogEntry {
    pub agent_type: AgentType,
    /// Kebab-case id (`plan-auditor`, …).
    pub slug: &'static str,
    /// PascalCase name for APIs and docs.
    pub display_name: &'static str,
    pub when_to_use: &'static str,
    pub max_react_loops: u32,
    /// Suggested tools in fork task prompt (not the LLM API tool filter).
    pub suggested_tools: &'static [&'static str],
    /// When `prompt/agents/*.md` is missing at build time.
    pub fallback_prompt: &'static str,
    /// Append to docs tools column (e.g. read-only note).
    pub tools_doc_suffix: &'static str,
    /// When true, docs note that `settings.agent.knowledge_auditor_max_react_loops` overrides default.
    pub loops_overridable_by_settings: bool,
}

const PLAN_AUDITOR_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "PlotGraph",
    "ForeshadowTracker",
    "CharacterSearch",
    "TrackingQuery",
    "RelationQuery",
    "Corkboard",
    "Tail",
    "Stats",
];

const KNOWLEDGE_AUDITOR_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "CharacterSearch",
    "PlotGraph",
    "Tail",
    "TrackingQuery",
    "RelationQuery",
    "ForeshadowTracker",
];

const CHAPTER_CRAFT_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "CharacterSearch",
    "Stats",
    "Tail",
    "TrackingQuery",
    "RelationQuery",
];

const GENERAL_PURPOSE_TOOLS: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Grep",
    "Glob",
    "CharacterSearch",
    "PlotGraph",
    "Tail",
    "Stats",
    "InvokeSkill",
    "ImpactAnalysis",
    "TodoWrite",
    "WebSearch",
];

/// Docs note for GeneralPurpose: full tool list is in catalog; runtime gate applies.
pub const GENERAL_PURPOSE_TOOLS_DOC_NOTE: &str =
    "LLM API 与主 Agent 同 schema；Write/Edit/TodoWrite 运行时门控拒绝";

/// Single source of truth for fork agent loops and suggested tool lists.
pub const FORK_AGENT_CATALOG: &[ForkAgentCatalogEntry] = &[
    ForkAgentCatalogEntry {
        agent_type: AgentType::PlanAuditor,
        slug: "plan-auditor",
        display_name: "PlanAuditor",
        when_to_use: "细纲完成后的计划结构质量审计（大纲对齐、伏笔密度、因果闭合、人物轮换、字数分配）",
        max_react_loops: PLAN_AUDITOR_MAX_REACT_LOOPS,
        suggested_tools: PLAN_AUDITOR_TOOLS,
        fallback_prompt: "你是细纲计划审计 Agent。只读检查大纲对齐、伏笔密度、因果闭合、人物轮换、字数分配、登记完整性，输出自然语言报告与「接下来」指引。",
        tools_doc_suffix: "（只读）",
        loops_overridable_by_settings: false,
    },
    ForkAgentCatalogEntry {
        agent_type: AgentType::KnowledgeAuditor,
        slug: "knowledge-auditor",
        display_name: "KnowledgeAuditor",
        when_to_use: "知识库遗漏扫描 + 设定一致性深度审计（只读报告）",
        max_react_loops: KNOWLEDGE_AUDITOR_MAX_REACT_LOOPS_DEFAULT,
        suggested_tools: KNOWLEDGE_AUDITOR_TOOLS,
        fallback_prompt: "你是知识库审计 Agent。只读检查正文执行忠实度与收尾完整性，输出自然语言报告与「接下来」指引。",
        tools_doc_suffix: "（只读）",
        loops_overridable_by_settings: true,
    },
    ForkAgentCatalogEntry {
        agent_type: AgentType::ChapterCraftAnalyzer,
        slug: "chapter-craft-analyzer",
        display_name: "ChapterCraftAnalyzer",
        when_to_use: "对话质量 + 叙事节奏 + 情感轨迹 + 设定一致性（只读报告）",
        max_react_loops: CHAPTER_CRAFT_ANALYZER_MAX_REACT_LOOPS,
        suggested_tools: CHAPTER_CRAFT_TOOLS,
        fallback_prompt: "你是章节文笔分析 Agent。分析对话、节奏、情感、设定一致性，输出自然语言报告。禁止 fork 与 JSON。",
        tools_doc_suffix: "",
        loops_overridable_by_settings: false,
    },
    ForkAgentCatalogEntry {
        agent_type: AgentType::GeneralPurpose,
        slug: "general-purpose",
        display_name: "GeneralPurpose",
        when_to_use: "一次性自定义任务：调研、批量整理、特殊分析",
        max_react_loops: GENERAL_PURPOSE_MAX_REACT_LOOPS,
        suggested_tools: GENERAL_PURPOSE_TOOLS,
        fallback_prompt: "你是只读通用子 Agent。严格按下方自定义任务执行；结论写在返回正文中。Write/Edit 被门控拒绝。禁止 fork。",
        tools_doc_suffix: "",
        loops_overridable_by_settings: false,
    },
];

pub fn catalog_entry(agent_type: AgentType) -> &'static ForkAgentCatalogEntry {
    FORK_AGENT_CATALOG
        .iter()
        .find(|e| e.agent_type == agent_type)
        .unwrap_or_else(|| panic!("missing catalog entry for {agent_type:?}"))
}

pub fn fallback_prompt(agent_type: AgentType) -> &'static str {
    catalog_entry(agent_type).fallback_prompt
}

pub fn system_prompt(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::PlanAuditor => include_str!("../../../../prompt/agents/plan-auditor.md"),
        AgentType::KnowledgeAuditor => {
            include_str!("../../../../prompt/agents/knowledge-auditor.md")
        }
        AgentType::ChapterCraftAnalyzer => {
            include_str!("../../../../prompt/agents/chapter-craft-analyzer.md")
        }
        AgentType::GeneralPurpose => include_str!("../../../../prompt/agents/general_purpose.md"),
    }
}

impl ForkAgentCatalogEntry {
    pub fn suggested_tools_line(&self) -> String {
        self.suggested_tools.join("/")
    }

    /// Tools column for `docs/crates/novel-core.md` §1.3.
    pub fn docs_tools_summary(&self) -> String {
        let line = self.suggested_tools_line();
        match self.agent_type {
            AgentType::GeneralPurpose => {
                format!("{line}；{GENERAL_PURPOSE_TOOLS_DOC_NOTE}")
            }
            _ if !self.tools_doc_suffix.is_empty() => format!("{line}{}", self.tools_doc_suffix),
            _ => line,
        }
    }

    /// max_react_loops column for `docs/crates/novel-core.md` §1.3.
    pub fn docs_max_react_loops(&self) -> String {
        if self.loops_overridable_by_settings {
            format!(
                "{}（settings `knowledge_auditor_max_react_loops` 可覆盖）",
                self.max_react_loops
            )
        } else {
            self.max_react_loops.to_string()
        }
    }

    pub fn to_definition(self) -> AgentDefinition {
        AgentDefinition {
            agent_type: self.agent_type,
            name: self.slug.into(),
            when_to_use: self.when_to_use.into(),
            system_prompt: system_prompt(self.agent_type).into(),
            max_react_loops: self.max_react_loops,
            tools: self.suggested_tools.iter().map(|t| (*t).into()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_config::FORKABLE_AGENT_TYPE_NAMES;

    #[test]
    fn catalog_names_match_forkable_const() {
        assert_eq!(FORK_AGENT_CATALOG.len(), FORKABLE_AGENT_TYPE_NAMES.len());
        for (entry, name) in FORK_AGENT_CATALOG
            .iter()
            .zip(FORKABLE_AGENT_TYPE_NAMES.iter())
        {
            assert_eq!(entry.display_name, *name);
        }
    }

    #[test]
    fn novel_core_md_table_matches_catalog() {
        let rows = [
            (
                AgentType::PlanAuditor,
                "30",
                "Read/Grep/PlotGraph/ForeshadowTracker/CharacterSearch/TrackingQuery/RelationQuery/Corkboard/Tail/Stats（只读）",
            ),
            (
                AgentType::KnowledgeAuditor,
                "40（settings `knowledge_auditor_max_react_loops` 可覆盖）",
                "Read/Grep/CharacterSearch/PlotGraph/Tail/TrackingQuery/RelationQuery/ForeshadowTracker（只读）",
            ),
            (
                AgentType::ChapterCraftAnalyzer,
                "25",
                "Read/Grep/CharacterSearch/Stats/Tail/TrackingQuery/RelationQuery",
            ),
            (
                AgentType::GeneralPurpose,
                "20",
                &format!(
                    "Read/Write/Edit/Grep/Glob/CharacterSearch/PlotGraph/Tail/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/WebSearch；{GENERAL_PURPOSE_TOOLS_DOC_NOTE}"
                ),
            ),
        ];
        for (agent, loops, tools) in rows {
            let entry = catalog_entry(agent);
            assert_eq!(entry.docs_max_react_loops(), loops);
            assert_eq!(entry.docs_tools_summary(), tools);
        }
    }

    #[test]
    fn definition_matches_catalog() {
        for entry in FORK_AGENT_CATALOG {
            let def = entry.to_definition();
            assert_eq!(def.max_react_loops, entry.max_react_loops);
            assert_eq!(def.tools.len(), entry.suggested_tools.len());
            for (a, b) in def.tools.iter().zip(entry.suggested_tools.iter()) {
                assert_eq!(a, *b);
            }
        }
    }
}
