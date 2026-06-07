//! Hardcoded partial reports when a sub-agent hits context limits.

pub const OVERFLOW_KIND_INPUT_REJECTED: &str = "请求被拒绝：输入 token 超出模型窗口";
pub const OVERFLOW_KIND_OUTPUT_TRUNCATED: &str = "末轮输出被截断：生成触达 token 上限";

/// Minimal partial report: task preview + overflow explanation only.
pub fn build_partial_report(agent_type: &str, task_preview: &str, overflow_kind: &str) -> String {
    format!(
        "[子 Agent 上下文超限: {agent_type}]\n\n\
         ## 任务\n\
         {task_preview}\n\n\
         ## 说明\n\
         子 Agent 因上下文达到模型上限已停止（{overflow_kind}）。\n\
         部分 tool 操作可能已写入磁盘，知识库可能处于中间状态。\n\
         请主 Agent 缩小任务范围后重新 ForkSubAgent，或自行 Read 相关文件后继续。"
    )
}

pub fn task_preview_120(task: &str) -> String {
    let preview: String = task.chars().take(120).collect();
    if task.chars().count() > 120 {
        format!("{preview}…")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_report_has_task_and_explanation() {
        let r = build_partial_report(
            "KnowledgeAuditor",
            "扫描第3章",
            OVERFLOW_KIND_INPUT_REJECTED,
        );
        assert!(r.contains("## 任务"));
        assert!(r.contains("扫描第3章"));
        assert!(r.contains(OVERFLOW_KIND_INPUT_REJECTED));
        assert!(!r.contains("已完成"));
    }

    #[test]
    fn task_preview_truncates_at_120() {
        let long = "a".repeat(200);
        let p = task_preview_120(&long);
        assert!(p.chars().count() <= 121);
    }
}
