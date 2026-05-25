use crate::{parse_frontmatter, CharacterFrontmatter, KnowledgeError, KnowledgeStore};
use regex::Regex;
use std::sync::OnceLock;

fn chapter_num_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)chapter[-_]?(\d+)|Ch(\d+)|第(\d+)章").expect("chapter regex")
    })
}

fn parse_chapter_num(s: &str) -> u32 {
    chapter_num_re()
        .captures(s)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .and_then(|m| m.as_str().parse().ok())
        })
        .unwrap_or(0)
}

/// Maintain knowledge/INDEX.md — list characters, plot files, chapters, progress.
pub fn rebuild_index(store: &KnowledgeStore) -> Result<String, KnowledgeError> {
    let mut lines = vec![
        "# 知识库索引".into(),
        String::new(),
        "## 人物".into(),
        "| 人物 | 状态 | 最后更新 |".into(),
        "|------|------|---------|".into(),
    ];

    let chars_dir = store.root.join("knowledge/characters");
    let mut char_count = 0usize;
    if chars_dir.exists() {
        for entry in std::fs::read_dir(&chars_dir).map_err(KnowledgeError::Io)? {
            let entry = entry.map_err(KnowledgeError::Io)?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") || name.starts_with('_') {
                continue;
            }
            char_count += 1;
            let rel = format!("knowledge/characters/{name}");
            let display = name.trim_end_matches(".md");
            let (status, last_update) = match store.read_file(&rel) {
                Ok(content) => match parse_frontmatter::<CharacterFrontmatter>(&content) {
                    Ok((fm, _)) => (format!("{:?}", fm.status).to_lowercase(), fm.last_update),
                    Err(_) => ("unknown".into(), "—".into()),
                },
                Err(_) => ("unknown".into(), "—".into()),
            };
            lines.push(format!(
                "| [{display}](characters/{name}) | {status} | {last_update} |"
            ));
        }
    }

    lines.push(String::new());
    lines.push("## 进度".into());
    let chapters_dir = store.root.join("chapters");
    let mut chapter_nums = Vec::new();
    if chapters_dir.exists() {
        for entry in std::fs::read_dir(&chapters_dir).map_err(KnowledgeError::Io)? {
            let entry = entry.map_err(KnowledgeError::Io)?;
            let fname = entry.file_name().to_string_lossy().to_string();
            let num = parse_chapter_num(&fname);
            if num > 0 {
                chapter_nums.push(num);
            }
        }
    }
    chapter_nums.sort_unstable();
    let latest = chapter_nums.last().copied().unwrap_or(0);
    lines.push(format!("- 人物卡: {char_count}"));
    lines.push(format!("- 已完成章节: {}", chapter_nums.len()));
    lines.push(format!("- 最新章节: Ch{latest}"));

    lines.push(String::new());
    lines.push("## 情节".into());
    for rel in [
        "knowledge/plot/大纲.md",
        "knowledge/plot/因果链.md",
        "knowledge/plot/伏笔追踪.md",
    ] {
        if store.root.join(rel).exists() {
            lines.push(format!("- [{rel}]({rel})"));
        }
    }
    lines.push(String::new());
    lines.push("## 共享设定".into());
    for rel in [
        "knowledge/shared-systems/背景设定.md",
        "knowledge/shared-systems/时间线.md",
        "knowledge/shared-systems/战力系统.md",
        "knowledge/shared-systems/功法技能.md",
        "knowledge/shared-systems/场景追踪.md",
        "knowledge/shared-systems/道具追踪.md",
        "knowledge/shared-systems/势力追踪.md",
    ] {
        if store.root.join(rel).exists() {
            lines.push(format!("- [{rel}]({rel})"));
        }
    }
    // ── Multi-world section ─────────────────────────────────────
    let worlds_dir = store.root.join("knowledge/worlds");
    if worlds_dir.is_dir() {
        if let Ok(world_entries) = std::fs::read_dir(&worlds_dir) {
            for world_entry in world_entries.flatten() {
                let Ok(ft) = world_entry.file_type() else { continue };
                if !ft.is_dir() { continue; }
                let world_name = world_entry.file_name().to_string_lossy().to_string();
                if world_name.starts_with('_') { continue; }
                let world_dir = worlds_dir.join(&world_name);

                lines.push(String::new());
                lines.push(format!("## 世界: {world_name}"));

                // Characters in this world
                let wchars_dir = world_dir.join("characters");
                if wchars_dir.is_dir() {
                    lines.push(String::new());
                    lines.push(format!("### 角色 ({world_name})"));
                    lines.push("| 人物 | 状态 | 最后更新 |".into());
                    lines.push("|------|------|---------|".into());
                    if let Ok(wc_entries) = std::fs::read_dir(&wchars_dir) {
                        for wc in wc_entries.flatten() {
                            let name = wc.file_name().to_string_lossy().to_string();
                            if !name.ends_with(".md") || name.starts_with('_') { continue; }
                            let display = name.trim_end_matches(".md");
                            let rel = format!("knowledge/worlds/{world_name}/characters/{name}");
                            let (status, last_update) = match store.read_file(&rel) {
                                Ok(content) => match parse_frontmatter::<CharacterFrontmatter>(&content) {
                                    Ok((fm, _)) => (format!("{:?}", fm.status).to_lowercase(), fm.last_update),
                                    Err(_) => ("unknown".into(), "—".into()),
                                },
                                Err(_) => ("unknown".into(), "—".into()),
                            };
                            lines.push(format!(
                                "| [{display}](worlds/{world_name}/characters/{name}) | {status} | {last_update} |"
                            ));
                        }
                    }
                }

                // World INDEX.md summary (first ~200 chars after frontmatter)
                let windex_path = world_dir.join("INDEX.md");
                if windex_path.exists() {
                    if let Ok(windex) = std::fs::read_to_string(&windex_path) {
                        let body = if let Some(end) = windex.find("\n---") {
                            &windex[end + 4..]
                        } else {
                            &windex
                        };
                        let excerpt: String = body
                            .lines()
                            .filter(|l| !l.starts_with("---") && !l.trim().is_empty())
                            .take(3)
                            .fold(String::new(), |a, l| a + l + "\n");
                        if !excerpt.trim().is_empty() {
                            lines.push(String::new());
                            lines.push(excerpt.trim().to_string());
                        }
                    }
                }
            }
        }
    }

    let content = lines.join("\n");
    store.write_file("knowledge/INDEX.md", &content)?;
    Ok(content)
}

pub fn ensure_index(store: &KnowledgeStore) -> Result<String, KnowledgeError> {
    let path = store.root.join("knowledge/INDEX.md");
    if path.exists() {
        store.read_file("knowledge/INDEX.md")
    } else {
        rebuild_index(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rebuild_creates_index() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/主角.md"),
            "---\nname: 主角\ncategory: human\nfirstAppearance: Ch1\nlastUpdate: Ch2\nstatus: alive\npovCharacter: true\n---\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        std::fs::write(tmp.path().join("chapters/chapter-001.md"), "正文").unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let idx = rebuild_index(&store).unwrap();
        assert!(idx.contains("主角"));
        assert!(idx.contains("进度"));
        assert!(idx.contains("Ch1"));
    }
}
