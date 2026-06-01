use crate::message_types::CompactionMessage;
#[cfg(test)]
use crate::RoleContent;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

fn chapter_num_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)chapter[-_]?(\d+)|Ch(\d+)|第(\d+)章").expect("valid chapter number regex")
    })
}

/// Level 1: replace old chapter full text in messages with outline summaries.
#[cfg(test)]
pub(crate) fn apply_level1_messages(
    messages: &mut [RoleContent],
    project_root: &Path,
    recent_chapters_full: usize,
) {
    let outline_dir = project_root.join("knowledge/plot/细纲");
    if !outline_dir.exists() {
        return;
    }
    let mut chapter_nums: Vec<u32> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&outline_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(n) = extract_chapter_num(&name) {
                chapter_nums.push(n);
            }
        }
    }
    chapter_nums.sort_unstable();
    if chapter_nums.len() <= recent_chapters_full {
        return;
    }
    let cutoff = chapter_nums[chapter_nums.len() - recent_chapters_full];
    for (_, content) in messages.iter_mut() {
        if content.len() < 2000 {
            continue;
        }
        if let Some(ch) = extract_chapter_num(content) {
            if ch < cutoff {
                if let Some(summary) = read_outline_summary(project_root, ch) {
                    *content = format!("[章节摘要 Ch{ch}] {summary}");
                }
            }
        }
    }
}

/// Level 1 on compaction messages (retain region only).
pub fn apply_level1_on_compaction_messages(
    messages: &mut [CompactionMessage],
    project_root: &Path,
    recent_chapters_full: usize,
) {
    let outline_dir = project_root.join("knowledge/plot/细纲");
    if !outline_dir.exists() {
        return;
    }
    let mut chapter_nums: Vec<u32> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&outline_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(n) = extract_chapter_num(&name) {
                chapter_nums.push(n);
            }
        }
    }
    chapter_nums.sort_unstable();
    if chapter_nums.len() <= recent_chapters_full {
        return;
    }
    let cutoff = chapter_nums[chapter_nums.len() - recent_chapters_full];
    for msg in messages.iter_mut() {
        if msg.content.len() < 2000 {
            continue;
        }
        if let Some(ch) = extract_chapter_num(&msg.content) {
            if ch < cutoff {
                if let Some(summary) = read_outline_summary(project_root, ch) {
                    msg.content = format!("[章节摘要 Ch{ch}] {summary}");
                }
            }
        }
    }
}

fn extract_chapter_num(s: &str) -> Option<u32> {
    chapter_num_re().captures(s).and_then(|c| {
        c.get(1)
            .or_else(|| c.get(2))
            .or_else(|| c.get(3))
            .and_then(|m| m.as_str().parse().ok())
    })
}

fn read_outline_summary(project_root: &Path, chapter: u32) -> Option<String> {
    let dir = project_root.join("knowledge/plot/细纲");
    let pattern = format!("chapter-{chapter:03}");
    let entries = std::fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.contains(&pattern) || name.contains(&format!("{chapter:03}")) {
            let text = std::fs::read_to_string(e.path()).ok()?;
            return extract_summary_section(&text);
        }
    }
    None
}

fn extract_summary_section(text: &str) -> Option<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') && line.contains("章节摘要") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with("## ") && !line.contains("章节摘要") {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !lines.is_empty() {
                    break;
                }
                continue;
            }
            lines.push(trimmed);
        }
    }
    if !lines.is_empty() {
        return Some(lines.join(" "));
    }
    for line in text.lines() {
        if line.contains("章节摘要") {
            return Some(line.trim().to_string());
        }
    }
    Some(text.lines().take(3).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn level1_replaces_old_chapter_content() {
        let tmp = TempDir::new().unwrap();
        let outline = tmp.path().join("knowledge/plot/细纲");
        std::fs::create_dir_all(&outline).unwrap();
        std::fs::write(
            outline.join("chapter-001-细纲.md"),
            "## 章节摘要\n主角入门测试。",
        )
        .unwrap();
        std::fs::write(outline.join("chapter-002-细纲.md"), "## 章节摘要\n第二幕。").unwrap();
        std::fs::write(outline.join("chapter-003-细纲.md"), "## 章节摘要\n第三幕。").unwrap();
        let long_old = format!("chapter-001 正文 {}", "字".repeat(2500));
        let mut msgs = vec![
            ("system".into(), "sys".into()),
            ("user".into(), long_old),
            ("user".into(), "chapter-003 新内容".into()),
        ];
        apply_level1_messages(&mut msgs, tmp.path(), 1);
        assert!(msgs[1].1.contains("章节摘要"));
        assert!(msgs[1].1.len() < 500);
    }
}
