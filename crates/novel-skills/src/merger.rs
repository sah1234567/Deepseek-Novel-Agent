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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_description_without_when_to_use() {
        assert_eq!(format_skill_listing_description("仙侠流派", ""), "仙侠流派");
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
}
