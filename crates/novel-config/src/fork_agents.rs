//! Fork-subagent identifiers and default ReAct limits (lowest-layer SSOT).
//!
//! Full tool lists and prompt paths live in `novel-core::agent::catalog`.

/// `ForkSubAgent` JSON `agent_type` enum values (PascalCase).
pub const FORKABLE_AGENT_TYPE_NAMES: &[&str] = &[
    "PlanAuditor",
    "KnowledgeAuditor",
    "ChapterCraftAnalyzer",
    "GeneralPurpose",
];

pub const PLAN_AUDITOR_MAX_REACT_LOOPS: u32 = 30;
pub const KNOWLEDGE_AUDITOR_MAX_REACT_LOOPS_DEFAULT: u32 = 40;
pub const CHAPTER_CRAFT_ANALYZER_MAX_REACT_LOOPS: u32 = 25;
pub const GENERAL_PURPOSE_MAX_REACT_LOOPS: u32 = 20;

pub fn is_forkable_agent_type(name: &str) -> bool {
    FORKABLE_AGENT_TYPE_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forkable_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in FORKABLE_AGENT_TYPE_NAMES {
            assert!(seen.insert(*name));
        }
    }
}
