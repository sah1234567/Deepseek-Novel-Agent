use crate::{text_util::truncate_chars, KnowledgeError, KnowledgeStore};
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

pub const AUDIT_STATUS_PATH: &str = "knowledge/meta/audit-status.md";

const STATUS_TABLE_HEADING: &str = "## 状态表";
const DEFAULT_TEMPLATE: &str = include_str!("../../../templates/knowledge/meta/audit-status.md");

/// Which audit column to update when a subagent completes (subset of fork agents; no GeneralPurpose).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    PlanAuditor,
    KnowledgeAuditor,
    ChapterCraftAnalyzer,
}

const AUDIT_KIND_BY_AGENT_NAME: &[(&str, AuditKind)] = &[
    ("PlanAuditor", AuditKind::PlanAuditor),
    ("KnowledgeAuditor", AuditKind::KnowledgeAuditor),
    ("ChapterCraftAnalyzer", AuditKind::ChapterCraftAnalyzer),
];

impl AuditKind {
    pub fn from_agent_name(name: &str) -> Option<Self> {
        AUDIT_KIND_BY_AGENT_NAME
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, k)| *k)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PlanAuditor => "细纲PA",
            Self::KnowledgeAuditor => "正文KA",
            Self::ChapterCraftAnalyzer => "文笔CCA",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditChapterRow {
    pub chapter: u32,
    pub plan_pa: String,
    pub body_ka: String,
    pub craft_cca: String,
    pub last_updated: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditStatusSummary {
    pub plan_pa_passed_through: Option<u32>,
    pub body_ka_passed_through: Option<u32>,
    pub craft_cca_passed_through: Option<u32>,
    pub pending_plan_pa: Vec<u32>,
    pub pending_body_ka: Vec<u32>,
    pub pending_craft_cca: Vec<u32>,
}

fn chapter_num_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)chapter[-_]?(\d+)|Ch(\d+)|第(\d+)章").expect("chapter regex")
    })
}

/// Extract unique sorted chapter numbers from free text (task paths, reports).
pub fn parse_chapter_numbers(text: &str) -> Vec<u32> {
    let mut nums: Vec<u32> = chapter_num_re()
        .captures_iter(text)
        .filter_map(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .and_then(|m| m.as_str().parse().ok())
        })
        .filter(|n| *n > 0)
        .collect();
    nums.sort_unstable();
    nums.dedup();
    nums
}

fn today_iso() -> String {
    // Stable date for tests: use chrono-free approach from local time when available.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // YYYY-MM-DD approximation without chrono — good enough for ledger stamps.
    let days = secs / 86_400;
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html (civil_from_days)
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mp < 10 { y } else { y + 1 };
    (y, m, d)
}

/// Create `knowledge/meta/audit-status.md` from template when missing (migration-on-read).
pub fn ensure_audit_status(store: &KnowledgeStore) -> Result<String, KnowledgeError> {
    match store.read_file(AUDIT_STATUS_PATH) {
        Ok(content) => Ok(content),
        Err(KnowledgeError::FileNotFound(_)) => {
            store.write_file(AUDIT_STATUS_PATH, DEFAULT_TEMPLATE)?;
            store.read_file(AUDIT_STATUS_PATH)
        }
        Err(e) => Err(e),
    }
}

fn parse_table_cells(line: &str) -> Option<Vec<String>> {
    if !line.starts_with('|') || line.contains("---") {
        return None;
    }
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 3 {
        return None;
    }
    let cells: Vec<String> = parts[1..parts.len().saturating_sub(1)]
        .iter()
        .map(|s| s.trim().to_string())
        .collect();
    if cells.is_empty() || cells[0].contains("示例") || cells[0].contains('章') {
        return None;
    }
    Some(cells)
}

fn parse_chapter_cell(cell: &str) -> Option<u32> {
    let n = parse_chapter_numbers(cell).into_iter().next()?;
    (n > 0).then_some(n)
}

fn parse_status_rows(content: &str) -> Vec<AuditChapterRow> {
    let Some(section_start) = content.find(STATUS_TABLE_HEADING) else {
        return vec![];
    };
    let section = &content[section_start..];
    let mut rows = Vec::new();
    for line in section.lines() {
        let Some(cells) = parse_table_cells(line) else {
            continue;
        };
        if cells.len() < 6 {
            continue;
        }
        let Some(ch) = parse_chapter_cell(&cells[0]) else {
            continue;
        };
        rows.push(AuditChapterRow {
            chapter: ch,
            plan_pa: cells[1].clone(),
            body_ka: cells[2].clone(),
            craft_cca: cells[3].clone(),
            last_updated: cells[4].clone(),
            note: cells.get(5).cloned().unwrap_or_default(),
        });
    }
    rows
}

fn format_chapter(ch: u32) -> String {
    format!("Ch{ch}")
}

fn upsert_status_cell(content: &str, chapter: u32, kind: AuditKind, status: &str) -> String {
    let date = today_iso();
    let mut rows = parse_status_rows(content);
    if let Some(row) = rows.iter_mut().find(|r| r.chapter == chapter) {
        match kind {
            AuditKind::PlanAuditor => row.plan_pa = status.into(),
            AuditKind::KnowledgeAuditor => row.body_ka = status.into(),
            AuditKind::ChapterCraftAnalyzer => row.craft_cca = status.into(),
        }
        row.last_updated = date;
    } else {
        let mut row = AuditChapterRow {
            chapter,
            plan_pa: "未审".into(),
            body_ka: "未审".into(),
            craft_cca: "未审".into(),
            last_updated: date.clone(),
            note: String::new(),
        };
        match kind {
            AuditKind::PlanAuditor => row.plan_pa = status.into(),
            AuditKind::KnowledgeAuditor => row.body_ka = status.into(),
            AuditKind::ChapterCraftAnalyzer => row.craft_cca = status.into(),
        }
        rows.push(row);
    }
    rows.sort_by_key(|r| r.chapter);
    rebuild_status_table(content, &rows)
}

fn rebuild_status_table(content: &str, rows: &[AuditChapterRow]) -> String {
    let (before, after) = split_status_section(content);
    let mut table = vec![
        STATUS_TABLE_HEADING.to_string(),
        String::new(),
        "| 章 | 细纲PA | 正文KA | 文笔CCA | 最后更新 | 备注 |".into(),
        "|----|--------|--------|---------|----------|------|".into(),
    ];
    for r in rows {
        table.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            format_chapter(r.chapter),
            r.plan_pa,
            r.body_ka,
            r.craft_cca,
            r.last_updated,
            r.note
        ));
    }
    let mut out = before;
    out.push_str(&table.join("\n"));
    out.push('\n');
    out.push_str(&after);
    out
}

fn split_status_section(content: &str) -> (String, String) {
    let start = content.find(STATUS_TABLE_HEADING).unwrap_or(content.len());
    let before = content[..start].to_string();
    // Status table only; trailing content after the table is dropped on rebuild.
    (before, String::new())
}

/// Mark chapters as `已审计` after a subagent report is injected.
pub fn mark_audited(
    store: &KnowledgeStore,
    kind: AuditKind,
    chapters: &[u32],
    task_snippet: &str,
) -> Result<(), KnowledgeError> {
    if chapters.is_empty() {
        return Ok(());
    }
    let mut content = ensure_audit_status(store)?;
    let note = truncate_chars(task_snippet, 80);
    for &ch in chapters {
        content = upsert_status_cell(&content, ch, kind, "已审计");
        if !note.is_empty() {
            let mut rows = parse_status_rows(&content);
            if let Some(r) = rows.iter_mut().find(|r| r.chapter == ch) {
                if r.note.is_empty() {
                    r.note = note.clone();
                }
            }
            content = rebuild_status_table(&content, &rows);
        }
    }
    store.write_file(AUDIT_STATUS_PATH, &content)
}

fn max_passed(rows: &[AuditChapterRow], pick: fn(&AuditChapterRow) -> &str) -> Option<u32> {
    rows.iter()
        .filter(|r| pick(r) == "已通过")
        .map(|r| r.chapter)
        .max()
}

fn pending_chapters(rows: &[AuditChapterRow], pick: fn(&AuditChapterRow) -> &str) -> Vec<u32> {
    rows.iter()
        .filter(|r| pick(r) != "已通过" && pick(r) != "不适用")
        .map(|r| r.chapter)
        .collect()
}

pub fn query_summary(store: &KnowledgeStore) -> Result<AuditStatusSummary, KnowledgeError> {
    let content = ensure_audit_status(store)?;
    let rows = parse_status_rows(&content);
    Ok(AuditStatusSummary {
        plan_pa_passed_through: max_passed(&rows, |r| &r.plan_pa),
        body_ka_passed_through: max_passed(&rows, |r| &r.body_ka),
        craft_cca_passed_through: max_passed(&rows, |r| &r.craft_cca),
        pending_plan_pa: pending_chapters(&rows, |r| &r.plan_pa),
        pending_body_ka: pending_chapters(&rows, |r| &r.body_ka),
        pending_craft_cca: pending_chapters(&rows, |r| &r.craft_cca),
    })
}

pub fn query_chapter(
    store: &KnowledgeStore,
    chapter: u32,
) -> Result<Option<AuditChapterRow>, KnowledgeError> {
    let content = ensure_audit_status(store)?;
    Ok(parse_status_rows(&content)
        .into_iter()
        .find(|r| r.chapter == chapter))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingFilter {
    PlanPa,
    BodyKa,
    CraftCca,
    Any,
}

pub fn list_pending(
    store: &KnowledgeStore,
    filter: PendingFilter,
) -> Result<Vec<u32>, KnowledgeError> {
    let summary = query_summary(store)?;
    let mut out: Vec<u32> = match filter {
        PendingFilter::PlanPa => summary.pending_plan_pa,
        PendingFilter::BodyKa => summary.pending_body_ka,
        PendingFilter::CraftCca => summary.pending_craft_cca,
        PendingFilter::Any => {
            let mut v = summary.pending_plan_pa;
            v.extend(summary.pending_body_ka);
            v.extend(summary.pending_craft_cca);
            v.sort_unstable();
            v.dedup();
            v
        }
    };
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Two-line progress hint for system prompt dynamic section.
pub fn format_progress_hint(store: &KnowledgeStore) -> Option<String> {
    let summary = query_summary(store).ok()?;
    let mut parts = Vec::new();
    if let Some(ch) = summary.plan_pa_passed_through {
        parts.push(format!("细纲PA已通过至 Ch{ch}"));
    }
    if let Some(ch) = summary.body_ka_passed_through {
        parts.push(format!("正文KA已通过至 Ch{ch}"));
    }
    if let Some(ch) = summary.craft_cca_passed_through {
        parts.push(format!("文笔CCA已通过至 Ch{ch}"));
    }
    if !summary.pending_plan_pa.is_empty() {
        parts.push(format!(
            "细纲PA待处理: {}",
            summary
                .pending_plan_pa
                .iter()
                .map(|c| format!("Ch{c}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("；"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn audit_kind_labels() {
        assert_eq!(AuditKind::PlanAuditor.label(), "细纲PA");
        assert_eq!(AuditKind::KnowledgeAuditor.label(), "正文KA");
        assert_eq!(AuditKind::ChapterCraftAnalyzer.label(), "文笔CCA");
    }

    #[test]
    fn audit_kind_from_agent_name() {
        assert_eq!(
            AuditKind::from_agent_name("PlanAuditor"),
            Some(AuditKind::PlanAuditor)
        );
        assert_eq!(AuditKind::from_agent_name("Unknown"), None);
    }

    #[test]
    fn parse_chapter_numbers_variants() {
        let nums = parse_chapter_numbers("审计 chapter-005 与 Ch3 和第12章");
        assert_eq!(nums, vec![3, 5, 12]);
    }

    #[test]
    fn ensure_creates_file() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        let content = ensure_audit_status(&store).expect("ensure");
        assert!(content.contains("状态表"));
        assert!(tmp.path().join(AUDIT_STATUS_PATH).exists());
    }

    #[test]
    fn mark_audited_upserts_rows() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        ensure_audit_status(&store).expect("ensure");
        mark_audited(
            &store,
            AuditKind::PlanAuditor,
            &[1, 2],
            "审计细纲 chapter-001",
        )
        .expect("mark");
        let row1 = query_chapter(&store, 1).expect("q").expect("row");
        assert_eq!(row1.plan_pa, "已审计");
        assert_eq!(row1.body_ka, "未审");
        let row2 = query_chapter(&store, 2).expect("q").expect("row");
        assert_eq!(row2.plan_pa, "已审计");
    }

    #[test]
    fn mark_audited_long_chinese_task_does_not_panic() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        ensure_audit_status(&store).expect("ensure");
        let long_task = format!(
            "请你fork subagent检查第一章{}",
            "场景细纲与正文对照审计".repeat(20)
        );
        mark_audited(&store, AuditKind::KnowledgeAuditor, &[1], &long_task).expect("mark");
        let row = query_chapter(&store, 1).expect("q").expect("row");
        assert_eq!(row.body_ka, "已审计");
        assert!(!row.note.is_empty());
        assert!(row.note.chars().count() <= 80);
    }

    #[test]
    fn list_pending_plan_pa() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        mark_audited(&store, AuditKind::PlanAuditor, &[1], "t").expect("mark");
        let pending = list_pending(&store, PendingFilter::PlanPa).expect("pending");
        assert!(pending.contains(&1));
    }
}
