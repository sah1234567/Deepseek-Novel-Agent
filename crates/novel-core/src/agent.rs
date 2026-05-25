use novel_config::AgentConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    LogIntegrityChecker,
    ConsistencyChecker,
    DialogueAnalyzer,
    PacingAnalyzer,
    EmotionAnalyzer,
    GeneralPurpose,
}

/// Agent types the main session may fork via `ForkSubAgent`.
pub const FORKABLE_AGENT_TYPE_NAMES: &[&str] = &[
    "ConsistencyChecker",
    "LogIntegrityChecker",
    "DialogueAnalyzer",
    "PacingAnalyzer",
    "EmotionAnalyzer",
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
    pub max_turns: u32,
    pub tools: Vec<String>,
}

impl AgentType {
    pub fn definition(self) -> AgentDefinition {
        match self {
            AgentType::LogIntegrityChecker => AgentDefinition {
                agent_type: self,
                name: "log-integrity-checker".into(),
                when_to_use: "扫描章节写后知识库更新遗漏，输出只读报告".into(),
                system_prompt: include_str!("../../../prompt/agents/log-integrity-checker.md").into(),
                max_turns: 15,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                ],
            },
            AgentType::ConsistencyChecker => AgentDefinition {
                agent_type: self,
                name: "consistency-checker".into(),
                when_to_use: "深度一致性审查（只读报告）".into(),
                system_prompt: include_str!("../../../prompt/agents/consistency_checker.md").into(),
                max_turns: 50,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                    "PlotGraph".into(),
                    "ChapterRead".into(),
                    "ConsistencyCheck".into(),
                ],
            },
            AgentType::DialogueAnalyzer => AgentDefinition {
                agent_type: self,
                name: "dialogue-analyzer".into(),
                when_to_use: "分析章节对话质量：标签重复、无归属对话、副词滥用、标签过长".into(),
                system_prompt: include_str!("../../../prompt/agents/dialogue-analyzer.md").into(),
                max_turns: 10,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                ],
            },
            AgentType::PacingAnalyzer => AgentDefinition {
                agent_type: self,
                name: "pacing-analyzer".into(),
                when_to_use: "分析章节节奏：对话/动作/叙述比例、节奏异常检测".into(),
                system_prompt: include_str!("../../../prompt/agents/pacing-analyzer.md").into(),
                max_turns: 10,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "Stats".into(),
                    "ChapterRead".into(),
                ],
            },
            AgentType::EmotionAnalyzer => AgentDefinition {
                agent_type: self,
                name: "emotion-analyzer".into(),
                when_to_use: "分析角色情感轨迹：情感转折点、突变检测、情感停滞警告".into(),
                system_prompt: include_str!("../../../prompt/agents/emotion-analyzer.md").into(),
                max_turns: 10,
                tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "CharacterSearch".into(),
                ],
            },
            AgentType::GeneralPurpose => AgentDefinition {
                agent_type: self,
                name: "general-purpose".into(),
                when_to_use: "一次性自定义任务：调研、批量整理、特殊分析".into(),
                system_prompt: include_str!("../../../prompt/agents/general_purpose.md").into(),
                max_turns: 20,
                tools: vec![
                    "Read".into(),
                    "Write".into(),
                    "Edit".into(),
                    "Grep".into(),
                    "Glob".into(),
                    "CharacterSearch".into(),
                    "PlotGraph".into(),
                    "ChapterRead".into(),
                    "Stats".into(),
                    "InvokeSkill".into(),
                    "ImpactAnalysis".into(),
                    "TodoWrite".into(),
                    "ConsistencyCheck".into(),
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

    pub fn max_turns(self) -> u32 {
        self.definition().max_turns
    }

    /// Max inner turns from settings when configured; fallback to `definition().max_turns`.
    pub fn max_turns_for(self, cfg: &AgentConfig) -> u32 {
        match self {
            AgentType::ConsistencyChecker => cfg.consistency_checker_max_turns,
            other => other.max_turns(),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "LogIntegrityChecker" | "log-integrity-checker" => Some(AgentType::LogIntegrityChecker),
            "ConsistencyChecker" | "consistency-checker" => Some(AgentType::ConsistencyChecker),
            "DialogueAnalyzer" | "dialogue-analyzer" => Some(AgentType::DialogueAnalyzer),
            "PacingAnalyzer" | "pacing-analyzer" => Some(AgentType::PacingAnalyzer),
            "EmotionAnalyzer" | "emotion-analyzer" => Some(AgentType::EmotionAnalyzer),
            "GeneralPurpose" | "general-purpose" => Some(AgentType::GeneralPurpose),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_config::AgentConfig;

    #[test]
    fn consistency_checker_has_consistency_check_not_edit() {
        let tools = AgentType::ConsistencyChecker.definition().tools;
        assert!(tools.contains(&"ConsistencyCheck".into()));
        assert!(!tools.contains(&"Edit".into()));
        assert!(!tools.contains(&"Write".into()));
    }

    #[test]
    fn log_integrity_checker_is_read_only() {
        let tools = AgentType::LogIntegrityChecker.definition().tools;
        assert!(tools.contains(&"Read".into()));
        assert!(!tools.contains(&"Edit".into()));
        assert!(!tools.contains(&"Write".into()));
    }

    #[test]
    fn max_turns_from_settings() {
        let cfg = AgentConfig {
            consistency_checker_max_turns: 42,
            ..AgentConfig::default()
        };
        assert_eq!(AgentType::ConsistencyChecker.max_turns_for(&cfg), 42);
    }

    #[test]
    fn general_purpose_has_write_and_consistency_check() {
        let tools = AgentType::GeneralPurpose.definition().tools;
        assert!(!tools.contains(&"ForkSubAgent".into()));
        assert!(tools.contains(&"ConsistencyCheck".into()));
        assert!(tools.contains(&"Write".into()));
    }

    #[test]
    fn forkable_names_include_log_integrity_checker() {
        assert!(AgentType::all_forkable_names().contains(&"LogIntegrityChecker"));
        assert!(AgentType::all_forkable_names().contains(&"GeneralPurpose"));
        assert!(AgentType::is_forkable("LogIntegrityChecker"));
        assert!(AgentType::is_forkable("ConsistencyChecker"));
    }

    #[test]
    fn removed_writer_types_not_parseable() {
        assert!(AgentType::parse("ChapterWriter").is_none());
        assert!(AgentType::parse("NovelPlanner").is_none());
        assert!(AgentType::parse("RevisionAgent").is_none());
    }
}
