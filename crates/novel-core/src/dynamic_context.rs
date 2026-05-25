use std::path::{Path, PathBuf};

use novel_knowledge::KnowledgeStore;
use novel_skills::load_skill;
use novel_state::Database;

use crate::system_prompt::DynamicContext;

/// Load memory/ index and referenced files (≤ max_bytes total).
pub fn load_memory(project_root: &Path, max_bytes: usize) -> String {
    let memory_dir = project_root.join("memory");
    let index_path = memory_dir.join("MEMORY.md");
    let index = std::fs::read_to_string(&index_path).unwrap_or_default();
    if index.is_empty() {
        return String::new();
    }
    let mut out = format!("{index}\n");
    if out.len() >= max_bytes {
        return truncate_str(&out, max_bytes);
    }
    for line in index.lines() {
        if let Some(name) = line.split(']').next().and_then(|s| s.strip_prefix('[')) {
            let p = memory_dir.join(format!("{name}.md"));
            if p.exists() {
                if let Ok(body) = std::fs::read_to_string(&p) {
                    out.push_str(&format!("\n### {name}\n{body}\n"));
                    if out.len() >= max_bytes {
                        return truncate_str(&out, max_bytes);
                    }
                }
            }
        }
    }
    truncate_str(&out, max_bytes)
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!(
            "{}…\n> WARNING: content was truncated ({} → {} chars). Only part of it was loaded.",
            &s[..max.saturating_sub(3)],
            s.len(),
            max
        )
    }
}

fn count_chapter_files(chapters_dir: &Path) -> u32 {
    if !chapters_dir.exists() {
        return 0;
    }
    std::fs::read_dir(chapters_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                .count() as u32
        })
        .unwrap_or(0)
}

fn outline_chapter_count(project_root: &Path) -> Option<u32> {
    let outline = project_root.join("knowledge/plot/大纲.md");
    let content = std::fs::read_to_string(outline).ok()?;
    let mut max_ch = 0u32;
    let mut chapter_idx: Option<usize> = None;
    for line in content.lines() {
        if !line.starts_with('|') {
            continue;
        }
        if line.contains("---") {
            continue;
        }
        let cells: Vec<String> = line.split('|').map(|s| s.trim().to_string()).collect();
        if cells.is_empty() {
            continue;
        }
        let first = cells.first().map(|s| s.as_str()).unwrap_or("");
        let is_header = matches!(
            first,
            "卷" | "章" | "章节" | "世界" | "副本" | "版本" | "所在世界" | "volume" | "chapter"
        );
        if is_header {
            for (i, c) in cells.iter().enumerate() {
                let key = c.to_lowercase();
                if key == "章" || key == "章节" || key == "chapter" {
                    chapter_idx = Some(i);
                    break;
                }
            }
            continue;
        }
        let idx = chapter_idx.unwrap_or(1);
        if let Some(ch) = cells.get(idx) {
            if let Ok(n) = ch.parse::<u32>() {
                max_ch = max_ch.max(n);
            }
        }
    }
    if max_ch > 0 {
        Some(max_ch)
    } else {
        None
    }
}

/// Build progress summary for system prompt dynamic layer.
pub fn load_progress(project_root: &Path, session_id: &str, db: &Database) -> String {
    let chapters_dir = project_root.join("chapters");
    let completed = count_chapter_files(&chapters_dir);
    let next = completed + 1;
    let total = outline_chapter_count(project_root);
    let unit_context = current_structure_unit(project_root);
    let todos = db.list_session_todos(session_id).unwrap_or_default();
    let in_progress: Vec<_> = todos
        .iter()
        .filter(|t| t.status == "in_progress")
        .map(|t| t.content.as_str())
        .collect();
    let mut lines = vec![
        format!("已完成章节文件: {completed}"),
        format!("下一章建议: Chapter {next}"),
    ];
    if let Some(t) = total {
        lines.push(format!("大纲计划章数: {t}"));
    }
    if !unit_context.is_empty() {
        lines.push(unit_context);
    }
    if !in_progress.is_empty() {
        lines.push(format!("进行中任务: {}", in_progress.join("; ")));
    }
    lines.join("\n")
}

fn current_structure_unit(project_root: &Path) -> String {
    let outline = project_root.join("knowledge/plot/大纲.md");
    let content = match std::fs::read_to_string(outline) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut last_heading = String::new();
    for line in content.lines().rev() {
        let line = line.trim();
        if line.starts_with("## ") {
            last_heading = line.trim_start_matches('#').trim().to_string();
            break;
        }
    }
    if last_heading.is_empty() {
        return String::new();
    }
    let unit_lower = last_heading.to_lowercase();
    let mut unit_chapter_count = 0u32;
    let mut in_target_unit = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("## ") {
            let heading = line.trim_start_matches('#').trim();
            in_target_unit = heading.to_lowercase() == unit_lower;
            continue;
        }
        if !in_target_unit || !line.starts_with('|') || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() >= 2 && !cells.is_empty() {
            let first = cells[0];
            if first == "卷" || first == "章节" || first == "世界" {
                continue;
            }
            unit_chapter_count += 1;
        }
    }
    if unit_chapter_count > 0 {
        format!("{last_heading}（本单元计划 {unit_chapter_count} 章）")
    } else {
        last_heading
    }
}

/// Assemble dynamic system prompt sections (Progress, Memory, INDEX, AGENTS, Skill summaries).
pub fn build_dynamic_context(
    project_root: &Path,
    session_id: &str,
    db: &Database,
    agents_md: &str,
    skills_dir: &Path,
) -> DynamicContext {
    let store = KnowledgeStore::new(project_root);
    let index = store
        .read_file("knowledge/INDEX.md")
        .unwrap_or_else(|_| "（索引尚未创建）".into());
    let project_skills = project_root.join("skills");
    let skills =
        novel_skills::load_skills_merged(&project_skills, skills_dir).unwrap_or_default();
    let skill_summaries: Vec<(String, String)> = skills
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                novel_skills::format_skill_listing_description(&s.description, &s.when_to_use),
            )
        })
        .collect();
    DynamicContext {
        agents_md: agents_md.to_string(),
        knowledge_index: index.chars().take(2000).collect(),
        memory: load_memory(project_root, 4096),
        progress: load_progress(project_root, session_id, db),
        skill_summaries,
        workspace_path: project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf())
            .display()
            .to_string(),
    }
}

fn normalize_read_path(project_root: &Path, read_path: &str) -> PathBuf {
    let normalized = read_path.replace('/', std::path::MAIN_SEPARATOR_STR);
    let p = PathBuf::from(normalized);
    if p.is_absolute() {
        p
    } else {
        project_root.join(p)
    }
}

fn match_reference_under_base(full: &Path, base: &Path) -> Option<(String, String)> {
    let rel = full.strip_prefix(base).ok()?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = rel_str.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let skill_id = parts[0];
    if skill_id.is_empty() || parts[1] != "references" {
        return None;
    }
    let filename = parts[2];
    if !filename.ends_with(".md") || filename == ".md" {
        return None;
    }
    let canonical = format!("{skill_id}/references/{filename}");
    Some((skill_id.to_string(), canonical))
}

/// Parse a Read tool path into `(skill_id, canonical)` when it points at
/// `skills/{id}/references/*.md` (project or agent bundle).
pub fn parse_skill_reference_path(
    project_root: &Path,
    agent_skills_dir: &Path,
    read_path: &str,
) -> Option<(String, String)> {
    let full = normalize_read_path(project_root, read_path);
    let project_skills = project_root.join("skills");
    for base in [&project_skills, agent_skills_dir] {
        if let Some(found) = match_reference_under_base(&full, base) {
            return Some(found);
        }
    }
    None
}

fn resolve_skill_reference_path(
    project_root: &Path,
    agent_skills_dir: &Path,
    canonical: &str,
) -> Option<PathBuf> {
    let parts: Vec<&str> = canonical.split('/').collect();
    if parts.len() != 3 || parts[1] != "references" || !parts[2].ends_with(".md") {
        return None;
    }
    let skill_id = parts[0];
    let filename = parts[2];
    let project_path = project_root
        .join("skills")
        .join(skill_id)
        .join("references")
        .join(filename);
    if project_path.exists() {
        return Some(project_path);
    }
    let agent_path = agent_skills_dir
        .join(skill_id)
        .join("references")
        .join(filename);
    if agent_path.exists() {
        return Some(agent_path);
    }
    None
}

/// Load reference markdown body from disk (project skills first, then agent bundle).
pub fn load_skill_reference_body(
    project_root: &Path,
    agent_skills_dir: &Path,
    canonical: &str,
) -> Option<String> {
    let path = resolve_skill_reference_path(project_root, agent_skills_dir, canonical)?;
    std::fs::read_to_string(path).ok()
}

fn resolve_skill_path(
    project_root: &Path,
    agent_skills_dir: &Path,
    skill_id: &str,
) -> Option<PathBuf> {
    let project_path = project_root
        .join("skills")
        .join(skill_id)
        .join("SKILL.md");
    if project_path.exists() {
        return Some(project_path);
    }
    let agent_path = agent_skills_dir.join(skill_id).join("SKILL.md");
    if agent_path.exists() {
        return Some(agent_path);
    }
    None
}

fn load_skill_body(
    project_root: &Path,
    agent_skills_dir: &Path,
    skill_id: &str,
) -> Option<String> {
    let path = resolve_skill_path(project_root, agent_skills_dir, skill_id)?;
    load_skill(&path).ok().map(|s| s.body)
}

/// Deduplicate skill IDs while preserving first-seen order.
pub fn dedupe_skill_ids(ids: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

/// Keep invoked skill IDs whose SKILL.md still exists (deleted skills are dropped silently).
pub fn filter_loadable_skill_ids(
    project_root: &Path,
    agent_skills_dir: &Path,
    invoked_skill_ids: &[String],
) -> Vec<String> {
    dedupe_skill_ids(invoked_skill_ids)
        .into_iter()
        .filter(|id| resolve_skill_path(project_root, agent_skills_dir, id).is_some())
        .collect()
}

/// Deduplicate reference canonical paths while preserving first-seen order.
pub fn dedupe_reference_paths(paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    paths
        .iter()
        .filter(|p| seen.insert(p.as_str()))
        .cloned()
        .collect()
}

/// Keep read reference paths whose file exists and parent skill was invoked.
pub fn filter_loadable_reference_paths(
    project_root: &Path,
    agent_skills_dir: &Path,
    reference_paths: &[String],
    invoked_skill_ids: &[String],
) -> Vec<String> {
    let invoked: std::collections::HashSet<String> =
        dedupe_skill_ids(invoked_skill_ids).into_iter().collect();
    dedupe_reference_paths(reference_paths)
        .into_iter()
        .filter(|canonical| {
            canonical
                .split('/')
                .next()
                .is_some_and(|id| invoked.contains(id))
                && resolve_skill_reference_path(project_root, agent_skills_dir, canonical)
                    .is_some()
        })
        .collect()
}

/// Grouped `[激活 Skill]` block: SKILL body then read references per skill.
/// Each skill body is prefixed with its canonical base directory path.
pub fn format_activated_skill_block(
    project_root: &Path,
    agent_skills_dir: &Path,
    skill_ids: &[String],
    reference_paths: &[String],
) -> String {
    let mut out = String::new();
    for skill_id in skill_ids.iter().rev() {
        if let Some(body) = load_skill_body(project_root, agent_skills_dir, skill_id) {
            // Prepend base directory prefix so LLM knows where to find reference files
            if let Some(skill_dir) =
                resolve_skill_dir(project_root, agent_skills_dir, skill_id)
            {
                let dir_str = skill_dir.display().to_string();
                out.push_str(&format!(
                    "> Skill 根目录: {dir_str}/\n> 引用文件请用此路径+相对路径拼接。\n\n"
                ));
            }
            out.push_str(&format!("\n### {skill_id}\n{body}\n"));
        }
        let prefix = format!("{skill_id}/references/");
        for canonical in reference_paths {
            if canonical.starts_with(&prefix) {
                if let Some(body) =
                    load_skill_reference_body(project_root, agent_skills_dir, canonical)
                {
                    out.push_str(&format!("\n### {canonical}\n{body}\n"));
                }
            }
        }
    }
    out
}

/// Resolve the canonical directory path for a skill.
fn resolve_skill_dir(
    project_root: &Path,
    agent_skills_dir: &Path,
    skill_id: &str,
) -> Option<PathBuf> {
    // Project-level override first
    let project_path = project_root.join("skills").join(skill_id);
    if project_path.join("SKILL.md").exists() {
        return Some(project_path.canonicalize().unwrap_or(project_path));
    }
    // Agent-level skill
    let agent_path = agent_skills_dir.join(skill_id);
    if agent_path.join("SKILL.md").exists() {
        return Some(agent_path.canonicalize().unwrap_or(agent_path));
    }
    None
}

/// Full SKILL.md bodies for invoked skills (post-compaction user message).
/// Missing or unreadable skills are skipped silently; caller should pass loadable IDs only.
pub fn format_invoked_skill_bodies(
    project_root: &Path,
    agent_skills_dir: &Path,
    invoked_skill_ids: &[String],
) -> String {
    format_activated_skill_block(project_root, agent_skills_dir, invoked_skill_ids, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_memory_empty_when_missing() {
        let tmp = TempDir::new().expect("tmp");
        assert!(load_memory(tmp.path(), 4096).is_empty());
    }

    #[test]
    fn load_progress_counts_chapters() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("chapters")).expect("dir");
        std::fs::write(tmp.path().join("chapters/chapter-001.md"), "x").expect("w");
        let db = Database::open(tmp.path().join("t.db")).expect("db");
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .expect("s");
        let p = load_progress(tmp.path(), &sid, &db);
        assert!(p.contains("已完成章节文件: 1"));
    }

    #[test]
    fn filter_loadable_skill_ids_drops_deleted_skills() {
        let tmp = TempDir::new().expect("tmp");
        let skill_dir = tmp.path().join("skills").join("kept");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: kept\ndescription: d\n---\n# ok\n")
            .expect("w");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let ids = filter_loadable_skill_ids(
            tmp.path(),
            &agent_skills,
            &["kept".into(), "removed".into(), "kept".into()],
        );
        assert_eq!(ids, vec!["kept"]);
    }

    #[test]
    fn format_invoked_skill_bodies_skips_missing_without_fallback() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["gone".into()]);
        assert!(bodies.is_empty());
    }

    #[test]
    fn format_invoked_skill_bodies_from_project_skills() {
        let tmp = TempDir::new().expect("tmp");
        let skill_dir = tmp.path().join("skills").join("xianxia");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        let marker = "仙侠写作规范全文内容不应截断";
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: xianxia\ndescription: d\n---\n# {marker}\n"),
        )
        .expect("w");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["xianxia".into()]);
        assert!(bodies.contains(marker));
    }

    #[test]
    fn dedupe_skill_ids_preserves_first_seen_order() {
        let ids = dedupe_skill_ids(&[
            "a".into(),
            "b".into(),
            "a".into(),
            "c".into(),
            "b".into(),
        ]);
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn format_invoked_skill_bodies_dedupes_ids() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        let skill_dir = agent_skills.join("apocalypse");
        std::fs::create_dir_all(&skill_dir).expect("dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: apocalypse\ndescription: d\n---\n# 末世规范\n",
        )
        .expect("w");
        let bodies = format_invoked_skill_bodies(tmp.path(), &agent_skills, &["apocalypse".into()]);
        assert!(bodies.contains("末世规范"));
    }

    #[test]
    fn build_dynamic_context_includes_progress() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("chapters")).expect("dir");
        let db = Database::open(tmp.path().join("t.db")).expect("db");
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .expect("s");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let ctx = build_dynamic_context(tmp.path(), &sid, &db, "agents", &agent_skills);
        assert!(ctx.progress.contains("已完成章节文件"));
    }

    fn write_reference(tmp: &TempDir, skill_id: &str, name: &str, body: &str) {
        let dir = tmp
            .path()
            .join("skills")
            .join(skill_id)
            .join("references");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(name), body).expect("w");
    }

    fn write_skill(tmp: &TempDir, skill_id: &str, body: &str) {
        let dir = tmp.path().join("skills").join(skill_id);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill_id}\ndescription: d\n---\n{body}\n"),
        )
        .expect("w");
    }

    #[test]
    fn parse_skill_reference_path_project_relative() {
        let tmp = TempDir::new().expect("tmp");
        write_reference(&tmp, "apocalypse", "zombie.md", "# zombie");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let parsed = parse_skill_reference_path(
            tmp.path(),
            &agent_skills,
            "skills/apocalypse/references/zombie.md",
        );
        assert_eq!(
            parsed,
            Some((
                "apocalypse".into(),
                "apocalypse/references/zombie.md".into()
            ))
        );
    }

    #[test]
    fn parse_skill_reference_path_agent_bundle() {
        let tmp = TempDir::new().expect("tmp");
        let agent_skills = tmp.path().join("agent_skills");
        let dir = agent_skills
            .join("romance")
            .join("references");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("harem.md"), "# h").expect("w");
        let abs = dir.join("harem.md");
        let parsed =
            parse_skill_reference_path(tmp.path(), &agent_skills, abs.to_str().unwrap());
        assert_eq!(
            parsed,
            Some(("romance".into(), "romance/references/harem.md".into()))
        );
    }

    #[test]
    fn parse_skill_reference_path_rejects_non_reference() {
        let tmp = TempDir::new().expect("tmp");
        write_skill(&tmp, "apocalypse", "# skill");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        assert!(parse_skill_reference_path(
            tmp.path(),
            &agent_skills,
            "skills/apocalypse/SKILL.md"
        )
        .is_none());
    }

    #[test]
    fn format_activated_skill_block_groups_references_under_skill() {
        let tmp = TempDir::new().expect("tmp");
        write_skill(&tmp, "apocalypse", "# apocalypse body");
        write_reference(&tmp, "apocalypse", "zombie.md", "# zombie ref");
        write_skill(&tmp, "romance", "# romance body");
        write_reference(&tmp, "romance", "harem.md", "# harem ref");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let refs = vec![
            "apocalypse/references/zombie.md".into(),
            "romance/references/harem.md".into(),
        ];
        let block = format_activated_skill_block(
            tmp.path(),
            &agent_skills,
            &["apocalypse".into(), "romance".into()],
            &refs,
        );
        let romance_pos = block.find("### romance\n").expect("skill2");
        let harem_pos = block.find("### romance/references/harem.md\n").expect("ref2");
        let apocalypse_pos = block.find("### apocalypse\n").expect("skill");
        let zombie_pos = block.find("### apocalypse/references/zombie.md\n").expect("ref");
        assert!(romance_pos < harem_pos);
        assert!(harem_pos < apocalypse_pos);
        assert!(apocalypse_pos < zombie_pos);
        assert!(block.contains("zombie ref"));
    }

    #[test]
    fn filter_loadable_reference_paths_requires_invoked_parent() {
        let tmp = TempDir::new().expect("tmp");
        write_reference(&tmp, "apocalypse", "zombie.md", "# z");
        write_reference(&tmp, "romance", "harem.md", "# h");
        let agent_skills = tmp.path().join("agent_skills");
        std::fs::create_dir_all(&agent_skills).expect("dir");
        let paths = vec![
            "apocalypse/references/zombie.md".into(),
            "romance/references/harem.md".into(),
        ];
        let filtered = filter_loadable_reference_paths(
            tmp.path(),
            &agent_skills,
            &paths,
            &["apocalypse".into()],
        );
        assert_eq!(filtered, vec!["apocalypse/references/zombie.md"]);
    }

    #[test]
    fn dedupe_reference_paths_preserves_order() {
        let paths = dedupe_reference_paths(&[
            "a/references/x.md".into(),
            "b/references/y.md".into(),
            "a/references/x.md".into(),
        ]);
        assert_eq!(
            paths,
            vec!["a/references/x.md", "b/references/y.md"]
        );
    }
}
