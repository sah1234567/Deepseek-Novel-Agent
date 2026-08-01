use crate::{
    derive_foreshadow_digest, find_table_last_row, parse_frontmatter, truncate_chars,
    CharacterFrontmatter, KnowledgeError, KnowledgeStore,
};
use std::collections::{BTreeMap, HashMap};

const SNAPSHOT_HEADING: &str = "## 当前状态快照";

/// Rebuild the `## 当前状态快照` section from evolution log last rows.
pub fn derive_character_snapshot(content: &str) -> Result<String, KnowledgeError> {
    let (fm, _body): (CharacterFrontmatter, _) = parse_frontmatter(content)?;
    let mut bullets = vec![format!("- 姓名: {}", fm.name)];

    for (label, table) in [
        ("身份", "身份演变日志"),
        ("修为", "修为演变日志"),
        ("性格", "性格演变日志"),
        ("最后出场", "出场记录日志"),
    ] {
        if let Ok(Some(row)) = find_table_last_row(content, table) {
            bullets.push(format!("- {label}: {row}"));
        }
    }

    let snapshot = format!("{SNAPSHOT_HEADING}\n{}", bullets.join("\n"));
    if let Some(start) = content.find(SNAPSHOT_HEADING) {
        let after_heading = &content[start + SNAPSHOT_HEADING.len()..];
        let end = after_heading
            .find("\n## ")
            .map(|i| start + SNAPSHOT_HEADING.len() + i)
            .unwrap_or(content.len());
        Ok(format!(
            "{}{}{}",
            &content[..start],
            snapshot,
            &content[end..]
        ))
    } else {
        let insert_at = content.find("\n## ").unwrap_or(content.len());
        Ok(format!(
            "{}\n\n{snapshot}{}",
            &content[..insert_at],
            &content[insert_at..]
        ))
    }
}

/// Group foreshadow entries by status category.
pub fn derive_foreshadow_categories(
    store: &KnowledgeStore,
) -> Result<HashMap<String, Vec<String>>, KnowledgeError> {
    let content = match store.read_file("knowledge/plot/伏笔追踪.md") {
        Ok(c) => c,
        Err(KnowledgeError::FileNotFound(_)) => return Ok(HashMap::new()),
        Err(e) => return Err(e),
    };

    let mut categories: HashMap<String, Vec<String>> = HashMap::new();
    for line in content.lines() {
        if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 6 {
            continue;
        }
        let id = cells[2].to_string();
        let status = cells[5].to_string();
        let key = if status.contains("待回收") {
            "pending".into()
        } else if status.contains("已回收") {
            "resolved".into()
        } else if status.contains("已废弃") {
            "abandoned".into()
        } else {
            "other".into()
        };
        categories.entry(key).or_default().push(id);
    }
    Ok(categories)
}

/// Scan character cards and rebuild `_关系与称呼索引.md` body from 关系演变日志 rows.
pub fn derive_relation_cross_index(store: &KnowledgeStore) -> Result<String, KnowledgeError> {
    let chars_dir = store.root.join("knowledge/characters");
    if !chars_dir.exists() {
        return Ok(String::new());
    }

    let mut rows: BTreeMap<(String, String), String> = BTreeMap::new();
    for entry in std::fs::read_dir(&chars_dir).map_err(KnowledgeError::Io)? {
        let entry = entry.map_err(KnowledgeError::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".md") || name.starts_with('_') {
            continue;
        }
        let speaker = name.trim_end_matches(".md").to_string();
        let rel = format!("knowledge/characters/{name}");
        let content = store.read_file(&rel)?;
        for line in content.lines() {
            if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
                continue;
            }
            let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
            if cells.len() < 5 {
                continue;
            }
            let chapter = cells[1].to_string();
            let target = cells[2].to_string();
            if target.is_empty() || target == "对象" {
                continue;
            }
            let old_rel = cells.get(3).copied().unwrap_or("—").to_string();
            let new_rel = cells.get(4).copied().unwrap_or("—").to_string();
            let title_change = cells.get(5).copied().unwrap_or("—").to_string();
            let reverse_title = cells.get(6).copied().unwrap_or("—").to_string();
            let trigger = cells.get(7).copied().unwrap_or("—").to_string();
            let row = format!(
                "| {chapter} | {speaker} | {target} | {old_rel} | {new_rel} | {title_change} | {reverse_title} | {trigger} |"
            );
            rows.insert((speaker.clone(), target), row);
        }
    }

    let mut out = vec![
        "# 关系与称呼索引".into(),
        String::new(),
        "## 关系演变日志".into(),
        "| 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |"
            .into(),
        "|------|--------|------|--------|--------|--------------|---------------|----------|"
            .into(),
    ];
    for (_, row) in rows {
        out.push(row);
    }
    Ok(out.join("\n"))
}

const WRITE_AFTER_HEADING: &str = "### 实际完成";
const CORE_EVENT_PREFIX: &str = "- 核心事件: ";

/// Extract the `- 核心事件: ` value from a fine outline's 写后记录 section.
fn extract_core_event(content: &str) -> Option<String> {
    let start = content.find(WRITE_AFTER_HEADING)?;
    let section = &content[start..];
    for line in section.lines().take(20) {
        if let Some(rest) = line.strip_prefix(CORE_EVENT_PREFIX) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(truncate_chars(trimmed, 40).to_string());
            }
        }
    }
    None
}

/// Extract the first bullet value of a character's `## 当前状态快照` section
/// (the derived snapshot's second line after `- 姓名: ...`).
fn extract_snapshot_brief(content: &str) -> Option<String> {
    let start = content.find(SNAPSHOT_HEADING)?;
    let section = &content[start..];
    for line in section.lines().skip(1).take(10) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("## ") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(truncate_chars(value, 30).to_string());
            }
        }
    }
    None
}

/// Derive a compact work-state digest (≤800 chars) for Progress injection.
///
/// Combines: recent chapter core events (fine-outline 写后记录, up to 3), current
/// character snapshots (up to 4), and the active-foreshadow digest. Best-effort:
/// missing files contribute nothing; `None` when there is nothing to show.
pub fn derive_work_digest(store: &KnowledgeStore, current_chapter: u32) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // 1. Recent chapter core events, newest first (up to 3).
    let mut events: Vec<String> = Vec::new();
    let first = current_chapter.saturating_sub(3);
    for ch in (first..current_chapter).rev() {
        let rel = format!("knowledge/plot/细纲/chapter-{ch:03}-细纲.md");
        if let Ok(content) = store.read_file(&rel) {
            if let Some(ev) = extract_core_event(&content) {
                events.push(format!("Ch{ch}: {ev}"));
            }
        }
        if events.len() >= 3 {
            break;
        }
    }
    if !events.is_empty() {
        parts.push(format!("最近章节: {}", events.join(" | ")));
    }

    // 2. Character snapshot briefs (up to 4, sorted by name).
    let chars_dir = store.root.join("knowledge/characters");
    if chars_dir.is_dir() {
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&chars_dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".md") && !fname.starts_with('_') {
                    names.push(fname.trim_end_matches(".md").to_string());
                }
            }
        }
        names.sort();
        let mut snaps: Vec<String> = Vec::new();
        for name in names.iter().take(4) {
            let rel = format!("knowledge/characters/{name}.md");
            if let Ok(content) = store.read_file(&rel) {
                if let Some(brief) = extract_snapshot_brief(&content) {
                    snaps.push(format!("{name}({brief})"));
                }
            }
        }
        if !snaps.is_empty() {
            parts.push(format!("人物状态: {}", snaps.join(" | ")));
        }
    }

    // 3. Active foreshadow digest (reuses derive_foreshadow_digest).
    if let Some(fd) = derive_foreshadow_digest(store, &format!("Ch{current_chapter}")) {
        parts.push(fd);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const CARD: &str = r#"---
name: 林若烟
aliases: []
category: human
firstAppearance: Ch1
lastUpdate: Ch3
status: alive
povCharacter: true
---

## 身份演变日志
| 章节 | 身份 | 触发事件 |
|------|------|---------|
| Ch3 | 内门弟子 | 考核通过 |

## 出场记录日志
| 章节 | 关键事件 | 伏笔 | 情绪 |
|------|---------|------|------|
| Ch3 | 修炼 | — | 专注 |
"#;

    #[test]
    fn derive_snapshot_inserts_section() {
        let updated = derive_character_snapshot(CARD).unwrap();
        assert!(updated.contains("当前状态快照"));
        assert!(updated.contains("内门弟子"));
        assert!(updated.contains("修炼"));
    }

    #[test]
    fn derive_foreshadow_groups_by_status() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/伏笔追踪.md"),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch1 | F01 | 埋设 | 伤疤 | 待回收 | Ch10 | 陈默 |\n\
             | Ch5 | F02 | 回收 | 戒指 | 已回收 | Ch5 | 林若烟 |\n",
        )
        .unwrap();
        let cats = derive_foreshadow_categories(&store).unwrap();
        assert!(cats
            .get("pending")
            .is_some_and(|v| v.contains(&"F01".to_string())));
        assert!(cats
            .get("resolved")
            .is_some_and(|v| v.contains(&"F02".to_string())));
    }

    #[test]
    fn work_digest_combines_events_characters_foreshadows() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        std::fs::create_dir_all(tmp.path().join("knowledge/plot/细纲")).unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/细纲/chapter-012-细纲.md"),
            "## 写后记录\n### 实际完成\n- 字数: 3120\n- 核心事件: 陈默在拍卖会拍得玉佩\n- 钩子: 苏雨桐认出玉佩\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/陈默.md"),
            "---\nname: 陈默\n---\n## 当前状态快照\n- 姓名: 陈默\n- 身份: 筑基中期\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/伏笔追踪.md"),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch8 | F07 | 埋设 | 玉佩 | 待回收 | Ch13 | 陈默 |\n",
        )
        .unwrap();
        let d = derive_work_digest(&store, 13).expect("digest");
        assert!(d.contains("Ch12"), "recent event missing: {d}");
        assert!(d.contains("拍卖会"), "event text missing: {d}");
        assert!(d.contains("陈默"), "character missing: {d}");
        assert!(d.contains("活跃伏笔"), "foreshadow missing: {d}");
    }

    #[test]
    fn work_digest_none_when_empty_project() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        assert!(derive_work_digest(&store, 1).is_none());
    }

    #[test]
    fn derive_relation_index_from_cards() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "## 关系演变日志\n| 章节 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |\n\
             |------|------|--------|--------|--------------|---------------|----------|\n\
             | Ch3 | 陈默 | 陌生 | 亲近 | —→\"陈前辈\" | —→\"丫头\" | 初见 |\n",
        )
        .unwrap();
        let idx = derive_relation_cross_index(&store).unwrap();
        assert!(idx.contains("林若烟"));
        assert!(idx.contains("陈默"));
    }
}
