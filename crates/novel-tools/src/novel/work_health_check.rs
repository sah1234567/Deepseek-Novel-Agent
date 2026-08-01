//! WorkHealthCheck — read-only aggregate health report across the whole work.
//!
//! Six aggregations:
//! 1. foreshadow recovery rate (≥60% target, audit-knowledge global standard)
//! 2. character rotation (last-appearance gaps >5 chapters)
//! 3. causality dangling edges (events nothing depends on)
//! 4. audit status (un-audited / pending chapters)
//! 5. REGATE iteration counts from graph-state.json (≥3 flagged; #11 merged)
//! 6. per-chapter word counts vs 2000–4000 target
//!
//! Pure function aggregation — no LLM cost. Output uses 严重/一般/建议 severity buckets
//! (AutoSci `check` grading style).

use super::common::{count_chinese_chars, list_chapter_files, parse_chapter_num};
use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{list_pending, parse_causality_markdown, KnowledgeStore};
use serde_json::{json, Value};

pub struct WorkHealthCheckTool;

const FORESHADOW_RECOVERY_TARGET: f32 = 0.60;
const ROTATION_WARN_CHAPTERS: u32 = 5;
const ITERATION_RED: u32 = 3;
const WORD_MIN: u32 = 2000;
const WORD_MAX: u32 = 4000;
const GRAPH_STATE_REL: &str = "knowledge/meta/graph-state.json";
const CAUSALITY_REL: &str = "knowledge/plot/因果链.md";

/// (pending, recovered, abandoned) row counts in the foreshadow tracking table.
fn foreshadow_counts(content: &str) -> (u32, u32, u32) {
    let mut pending = 0u32;
    let mut recovered = 0u32;
    let mut abandoned = 0u32;
    let table_start = content.find('|').unwrap_or(content.len());
    let table_section = &content[table_start..];
    let table_end = table_section.find("\n## ").unwrap_or(table_section.len());
    for line in table_section[..table_end].lines() {
        if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 8 {
            continue;
        }
        let status = cells[5];
        if status.contains("待回收") {
            pending += 1;
        } else if status.contains("已回收") {
            recovered += 1;
        } else if status.contains("已废弃") {
            abandoned += 1;
        }
    }
    (pending, recovered, abandoned)
}

/// Nodes with ≥3 REGATE iterations from graph-state.json.
fn regate_stats(project_root: &std::path::Path) -> Vec<String> {
    let path = project_root.join(GRAPH_STATE_REL);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(nodes) = v.get("nodes").and_then(|n| n.as_object()) {
        for (id, node) in nodes {
            let iter = node.get("iteration").and_then(|i| i.as_u64()).unwrap_or(0);
            if iter >= ITERATION_RED.into() {
                out.push(format!("{id}(iteration={iter})"));
            }
        }
    }
    out.sort();
    out
}

/// Characters whose last appearance is >5 chapters before the current one.
fn rotation_warnings(store: &KnowledgeStore, current_chapter: u32) -> Vec<String> {
    let chars_dir = store.root.join("knowledge/characters");
    let Ok(entries) = std::fs::read_dir(&chars_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if !fname.ends_with(".md") || fname.starts_with('_') {
            continue;
        }
        let name = fname.trim_end_matches(".md");
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(Some(row)) = novel_knowledge::find_table_last_row(&content, "出场记录日志")
        else {
            continue;
        };
        let last_ch = parse_chapter_num(&row);
        if last_ch > 0 && current_chapter.saturating_sub(last_ch) > ROTATION_WARN_CHAPTERS {
            out.push(format!("{name}(最近出场 Ch{last_ch})"));
        }
    }
    out.sort();
    out
}

fn word_distribution(project_root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for (ch, path) in list_chapter_files(project_root) {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let wc = count_chinese_chars(&content);
        if !(WORD_MIN..=WORD_MAX).contains(&wc) {
            out.push(format!("Ch{ch}({wc}字)"));
        }
    }
    out
}

/// One health item with a severity bucket.
struct HealthItem {
    /// "red" | "yellow" | "blue"
    severity: &'static str,
    text: String,
}

fn foreshadow_items(store: &KnowledgeStore) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let Ok(content) = store.read_file("knowledge/plot/伏笔追踪.md") else {
        return out;
    };
    let (pending, recovered, abandoned) = foreshadow_counts(&content);
    let total = pending + recovered + abandoned;
    if total == 0 {
        return out;
    }
    let rate = recovered as f32 / total as f32;
    if rate < FORESHADOW_RECOVERY_TARGET {
        out.push(HealthItem {
            severity: "red",
            text: format!(
                "伏笔回收率 {:.0}% < 60% (待回收 {pending} / 已回收 {recovered} / 已废弃 {abandoned})",
                rate * 100.0
            ),
        });
    } else {
        out.push(HealthItem {
            severity: "blue",
            text: format!(
                "伏笔回收率 {:.0}% (待回收 {pending} / 已回收 {recovered})",
                rate * 100.0
            ),
        });
    }
    out
}

fn rotation_items(store: &KnowledgeStore, current_chapter: u32) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let rotations = rotation_warnings(store, current_chapter);
    if !rotations.is_empty() {
        out.push(HealthItem {
            severity: "yellow",
            text: format!(
                "重要角色超过 {ROTATION_WARN_CHAPTERS} 章未出场: {}",
                rotations.join(", ")
            ),
        });
    }
    out
}

fn causality_items(store: &KnowledgeStore) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let Ok(content) = store.read_file(CAUSALITY_REL) else {
        return out;
    };
    let graph = parse_causality_markdown(&content);
    let isolated: Vec<String> = graph
        .node_ids()
        .into_iter()
        .filter(|id| graph.incoming_edges(id) == 0 && graph.outgoing_edges(id) == 0)
        .collect();
    if !isolated.is_empty() {
        out.push(HealthItem {
            severity: "red",
            text: format!("因果链孤立事件(无前因无后果): {}", isolated.join(", ")),
        });
    }
    let sink: Vec<String> = graph
        .node_ids()
        .into_iter()
        .filter(|id| graph.incoming_edges(id) > 0 && graph.outgoing_edges(id) == 0)
        .collect();
    if !sink.is_empty() {
        out.push(HealthItem {
            severity: "blue",
            text: format!(
                "因果链无后继事件(可能为正常终点或未闭合，收尾阶段应确认): {}",
                sink.join(", ")
            ),
        });
    }
    out
}

fn audit_items(store: &KnowledgeStore) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let Ok(pending_list) = list_pending(store, novel_knowledge::PendingFilter::Any) else {
        return out;
    };
    if !pending_list.is_empty() {
        let chs: Vec<String> = pending_list.iter().map(|c| format!("Ch{c}")).collect();
        out.push(HealthItem {
            severity: "yellow",
            text: format!("审计未通过/未审章节: {}", chs.join(", ")),
        });
    }
    out
}

fn regate_items(project_root: &std::path::Path) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let regates = regate_stats(project_root);
    if !regates.is_empty() {
        out.push(HealthItem {
            severity: "red",
            text: format!(
                "节点 REGATE ≥{ITERATION_RED} 次(建议人工审批): {}",
                regates.join(", ")
            ),
        });
    }
    out
}

fn word_items(project_root: &std::path::Path) -> Vec<HealthItem> {
    let mut out = Vec::new();
    let words = word_distribution(project_root);
    if !words.is_empty() {
        out.push(HealthItem {
            severity: "yellow",
            text: format!("字数偏离 2000-4000: {}", words.join(", ")),
        });
    }
    out
}

fn build_report(store: &KnowledgeStore, current_chapter: u32) -> String {
    let mut items = foreshadow_items(store);
    items.extend(rotation_items(store, current_chapter));
    items.extend(causality_items(store));
    items.extend(audit_items(store));
    items.extend(regate_items(&store.root));
    items.extend(word_items(&store.root));

    let mut lines = vec!["## WorkHealthCheck".to_string()];
    if items.is_empty() {
        lines.push("(作品健康，无告警)".to_string());
        return lines.join("\n");
    }
    let buckets = [("red", "严重"), ("yellow", "一般"), ("blue", "建议")];
    for (severity, label) in buckets {
        let in_bucket: Vec<&HealthItem> = items.iter().filter(|i| i.severity == severity).collect();
        if in_bucket.is_empty() {
            continue;
        }
        lines.push(format!("{label}:"));
        for item in in_bucket {
            lines.push(format!("- {}", item.text));
        }
    }
    lines.join("\n")
}

#[async_trait]
impl Tool for WorkHealthCheckTool {
    fn name(&self) -> &str {
        "WorkHealthCheck"
    }
    fn description(&self) -> &str {
        "Aggregate whole-work health report (foreshadow recovery, character rotation, causality dangling edges, audit status, REGATE iterations, word counts) — 严重/一般/建议 severity buckets, read-only"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "current_chapter": {"type": "string", "description": "Chapter to measure against, e.g. \"Ch15\". Defaults to next unwritten chapter."}
            },
            "required": []
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let store = KnowledgeStore::new(&ctx.project_root);
        let current_chapter = input
            .get("current_chapter")
            .and_then(|v| v.as_str())
            .map(parse_chapter_num)
            .filter(|n| *n > 0)
            .unwrap_or_else(|| {
                super::common::list_chapter_files(&ctx.project_root).len() as u32 + 1
            });
        let report = build_report(&store, current_chapter);
        Ok(ToolOutput {
            content: report,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    fn write(root: &std::path::Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn ctx_for(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flags_low_recovery_and_rotation() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "knowledge/plot/伏笔追踪.md",
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch1 | F01 | 埋设 | 伤疤 | 待回收 | Ch20 | 陈默 |\n\
             | Ch2 | F02 | 埋设 | 玉佩 | 待回收 | Ch20 | 林若烟 |\n\
             | Ch5 | F03 | 回收 | 戒指 | 已回收 | Ch5 | 苏雨桐 |\n",
        );
        write(
            tmp.path(),
            "knowledge/characters/陈默.md",
            "---\nname: 陈默\n---\n## 出场记录日志\n| 章节 | 事件 |\n|------|------|\n| Ch2 | 出场 |\n",
        );
        write(
            tmp.path(),
            "knowledge/plot/因果链.md",
            "| 章节 | 事件 | 前因 | 后果 |\n|------|------|------|------|\n| Ch1 | 陈默捡到玉佩 | — | — |\n",
        );
        write(
            tmp.path(),
            "knowledge/meta/graph-state.json",
            r#"{"nodes":{"write-ch-1":{"iteration":4,"status":"AwaitingApproval"}}}"#,
        );
        write(
            tmp.path(),
            "knowledge/meta/audit-status.md",
            "| 章 | 细纲PA | 正文KA | 文笔CCA | 最后更新 | 备注 |\n\
             | Ch1 | 未审 | 未审 | 未审 | 2026-01-01 | |\n",
        );

        let tool = WorkHealthCheckTool;
        let out = tool
            .call(json!({"current_chapter": "Ch10"}), &ctx_for(&tmp))
            .await
            .unwrap();
        assert!(
            out.content.contains("严重:"),
            "should have red: {}",
            out.content
        );
        assert!(out.content.contains("回收率"), "recovery: {}", out.content);
        assert!(
            out.content.contains("iteration=4"),
            "regate: {}",
            out.content
        );
        assert!(out.content.contains("陈默"), "rotation: {}", out.content);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn healthy_work_reports_clean() {
        let tmp = TempDir::new().unwrap();
        let tool = WorkHealthCheckTool;
        let out = tool.call(json!({}), &ctx_for(&tmp)).await.unwrap();
        assert!(
            out.content.contains("无告警"),
            "empty project should be clean: {}",
            out.content
        );
    }

    #[test]
    fn foreshadow_counts_bucket_statuses() {
        let body = "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
                    |------|--------|------|---------|------|-----------|----------|\n\
                    | Ch1 | F01 | 埋设 | a | 待回收 | Ch20 | 陈默 |\n\
                    | Ch2 | F02 | 回收 | b | 已回收 | Ch2 | 林若烟 |\n\
                    | Ch3 | F03 | 废弃 | c | 已废弃 | — | 苏雨桐 |\n";
        assert_eq!(foreshadow_counts(body), (1, 1, 1));
    }
}
