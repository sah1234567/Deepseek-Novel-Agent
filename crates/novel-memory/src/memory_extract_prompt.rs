//! Memory extraction prompt builder.
//!
//! Loads the Chinese task template from `prompt/memory/extraction-task.md`.

const EXTRACTION_TASK_TEMPLATE: &str = include_str!("../../../prompt/memory/extraction-task.md");

/// Build the memory extraction task body (passed to `format_fork_task` as user task).
pub fn build_memory_extraction_prompt(
    new_message_count: usize,
    existing_memories_manifest: &str,
    memory_dir: &str,
) -> String {
    let existing_section = format_existing_memories_section(existing_memories_manifest);
    EXTRACTION_TASK_TEMPLATE
        .replace("{new_message_count}", &new_message_count.to_string())
        .replace("{existing_memories_section}", &existing_section)
        .replace("{memory_dir}", memory_dir)
}

fn format_existing_memories_section(manifest: &str) -> String {
    if manifest.is_empty() {
        return "（尚无 memory 文件）\n\n写入前先查此清单——优先**更新**已有文件，避免重复建文件。"
            .to_string();
    }
    format!(
        "写入前先查此清单——优先**更新**已有文件，避免重复建文件。\n\n\
         ### 关于 deprecated 文件\n\
         - deprecated 是已被推翻的旧决策——**不要**重新激活或基于它们写新 memory\n\
         - 除非用户明确说「恢复之前的方案」，否则跳过 deprecated 文件\n\
         - deprecated 文件保留不删——表示「该方案已否决」\n\n\
         {manifest}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_all_sections() {
        let prompt = build_memory_extraction_prompt(15, "", "memory");
        assert!(prompt.contains("15"));
        assert!(prompt.contains("尚无 memory 文件"));
        assert!(prompt.contains("style"));
        assert!(prompt.contains("plot_decision"));
        assert!(prompt.contains("不要保存"));
        assert!(prompt.contains("memory/"));
        assert!(prompt.contains("ReAct 循环预算"));
        assert!(prompt.contains("并行"));
        assert!(prompt.contains("禁止 grep 源码"));
    }

    #[test]
    fn build_prompt_includes_existing_manifest() {
        let manifest =
            "[style] style/pacing.md: 节奏偏好\n[plot_decision] plot_decisions/cp.md: CP设定\n";
        let prompt = build_memory_extraction_prompt(10, manifest, "memory");
        assert!(prompt.contains("pacing.md"));
        assert!(prompt.contains("cp.md"));
        assert!(prompt.contains("deprecated"));
    }

    #[test]
    fn opener_explicitly_says_do_nothing_when_no_content() {
        let prompt = build_memory_extraction_prompt(5, "", "memory");
        assert!(prompt.contains("什么都不做"));
    }
}
