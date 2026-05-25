use crate::SkillDefinition;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

fn path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:shared-systems|knowledge)/[^\s`（(]+").expect("valid skill path regex")
    })
}

/// Format a skill's listing line for system prompt injection (Claude Code parity).
///
/// When `when_to_use` is present and not already covered by `description`,
/// appends it as `" - {when_to_use}"`. Skips redundant append when the fields
/// are equal or `description` already contains `when_to_use`.
pub fn format_skill_listing_description(description: &str, when_to_use: &str) -> String {
    let desc = description.trim();
    let when = when_to_use.trim();
    if when.is_empty() {
        return desc.to_string();
    }
    if desc.is_empty() {
        return when.to_string();
    }
    if desc == when || desc.contains(when) {
        return desc.to_string();
    }
    format!("{desc} - {when}")
}

/// Merge optional knowledge file requirements from multiple skills (deduplicated).
pub fn merge_skill_requirements(skills: &[SkillDefinition]) -> Vec<String> {
    let re = path_re();
    let mut files = BTreeSet::new();
    for skill in skills {
        for cap in re.find_iter(&skill.body) {
            files.insert(cap.as_str().trim_end_matches('`').to_string());
        }
    }
    files.into_iter().collect()
}

/// Extract knowledge file paths referenced in a raw skill body text.
pub fn extract_skill_body_requirements(body: &str) -> Vec<String> {
    let re = path_re();
    let mut files = BTreeSet::new();
    for cap in re.find_iter(body) {
        files.insert(cap.as_str().trim_end_matches('`').to_string());
    }
    files.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn listing_description_without_when_to_use() {
        assert_eq!(
            format_skill_listing_description("仙侠流派", ""),
            "仙侠流派"
        );
    }

    #[test]
    fn listing_description_merges_distinct_when_to_use() {
        assert_eq!(
            format_skill_listing_description("Post-change checklist", "After any code edit"),
            "Post-change checklist - After any code edit"
        );
    }

    #[test]
    fn listing_description_skips_redundant_when_to_use() {
        let desc = "仙侠/修真流派——当用户要写仙侠题材时使用。触发词：\"仙侠\"";
        let when = "当用户要写仙侠题材时使用。触发词：\"仙侠\"";
        assert_eq!(format_skill_listing_description(desc, when), desc);
    }

    #[test]
    fn listing_description_uses_when_to_use_when_description_empty() {
        assert_eq!(
            format_skill_listing_description("", "Use for rebirth plots"),
            "Use for rebirth plots"
        );
    }

    #[test]
    fn merge_deduplicates() {
        let skills = vec![
            SkillDefinition {
                id: "a".into(),
                name: "a".into(),
                description: "d".into(),
                when_to_use: "w".into(),
                body: "需要 shared-systems/战力系统.md".into(),
                path: PathBuf::from("a.md"),
            },
            SkillDefinition {
                id: "b".into(),
                name: "b".into(),
                description: "d".into(),
                when_to_use: "w".into(),
                body: "可选 shared-systems/战力系统.md 和 shared-systems/场景追踪.md".into(),
                path: PathBuf::from("b.md"),
            },
        ];
        let merged = merge_skill_requirements(&skills);
        assert!(merged.contains(&"shared-systems/战力系统.md".to_string()));
        assert!(merged.contains(&"shared-systems/场景追踪.md".to_string()));
        assert_eq!(merged.len(), 2);
    }
}
