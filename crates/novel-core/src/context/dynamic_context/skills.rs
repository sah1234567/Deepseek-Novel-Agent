use std::path::{Path, PathBuf};

use novel_skills::load_skill;

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
    let full = novel_tools::resolve_under_project(project_root, read_path);
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
    let project_path = project_root.join("skills").join(skill_id).join("SKILL.md");
    if project_path.exists() {
        return Some(project_path);
    }
    let agent_path = agent_skills_dir.join(skill_id).join("SKILL.md");
    if agent_path.exists() {
        return Some(agent_path);
    }
    None
}

fn load_skill_body(project_root: &Path, agent_skills_dir: &Path, skill_id: &str) -> Option<String> {
    let path = resolve_skill_path(project_root, agent_skills_dir, skill_id)?;
    load_skill(&path).ok().map(|s| s.body)
}

/// Deduplicate skill IDs while preserving first-seen order.
pub(crate) fn dedupe_skill_ids(ids: &[String]) -> Vec<String> {
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
pub(crate) fn dedupe_reference_paths(paths: &[String]) -> Vec<String> {
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
                && resolve_skill_reference_path(project_root, agent_skills_dir, canonical).is_some()
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
            if let Some(skill_dir) = resolve_skill_dir(project_root, agent_skills_dir, skill_id) {
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
