/// Trailing user message appended after the cached session prefix for KV-cache-aware summarization.
pub fn build_summary_trailing_user_prompt() -> String {
    "[压缩摘要请求]\n\n\
     请将你看到的以上全部对话历史压缩为结构化摘要，供后续创作继续。严格按下列章节输出（某节无内容则写「无」）：\n\n\
     ## 创作进度\n\
     当前章节/阶段、写作或改稿进行到哪一步\n\n\
     ## 情节与正文\n\
     已写章节的关键事件、章末钩子、未回收伏笔\n\n\
     ## 设定与知识库\n\
     世界观/人物/关系/功法等变更；已更新或读取的 knowledge/ 路径\n\n\
     ## 关键操作\n\
     重要的 Write/Edit/Read/InvokeSkill/ForkSubAgent 及结果（一句话/条）\n\n\
     ## 作者意图与待办\n\
     用户明确要求、待确认项、进行中的 Todo\n\n\
     ## 子 Agent 结论\n\
     ConsistencyChecker / LogIntegrityChecker / 分析类 subagent 报告要点（若有）\n\n\
     输出要求：\n\
     - 只输出摘要正文（Markdown），不要前言或「以下是摘要」类套话\n\
     - 使用简体中文，保留章节号、人物名、文件路径等可操作细节\n\
     - 总长度控制在10000字左右，在保留可操作细节的前提下尽量精炼"
        .to_string()
}

/// Truncate summary text to max_chars (character count).
pub fn truncate_summary(summary: &str, max_chars: usize) -> String {
    if summary.chars().count() <= max_chars {
        summary.to_string()
    } else {
        format!(
            "{}…",
            summary
                .chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_prompt_has_marker_and_no_middle_placeholder() {
        let p = build_summary_trailing_user_prompt();
        assert!(p.contains("[压缩摘要请求]"));
        assert!(!p.contains("{middle_text}"));
        assert!(p.contains("10000字左右"));
        assert!(!p.contains("硬上限"));
    }

    #[test]
    fn truncate_summary_respects_limit() {
        let s = "字".repeat(20);
        let out = truncate_summary(&s, 10);
        assert!(out.chars().count() <= 10);
    }
}
