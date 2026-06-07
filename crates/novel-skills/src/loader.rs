use crate::SkillError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    when_to_use: String,
}

/// Parse a markdown file with YAML frontmatter into a SkillDefinition.
fn parse_skill_file(path: &Path, id: &str) -> Result<SkillDefinition, SkillError> {
    let content = std::fs::read_to_string(path)?;
    if !content.starts_with("---") {
        return Err(SkillError::MissingFrontmatter(path.display().to_string()));
    }
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return Err(SkillError::MissingFrontmatter(path.display().to_string()));
    }
    let fm: SkillFrontmatter = serde_yaml::from_str(parts[1].trim())?;
    Ok(SkillDefinition {
        id: id.to_string(),
        name: fm.name,
        description: fm.description,
        when_to_use: fm.when_to_use,
        body: parts[2].trim().to_string(),
        path: path.to_path_buf(),
    })
}

/// Load a skill from a SKILL.md file path. Used by InvokeSkill.
pub fn load_skill(path: impl AsRef<Path>) -> Result<SkillDefinition, SkillError> {
    let path = path.as_ref();
    let id = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("unknown")
        .to_string();
    parse_skill_file(path, &id)
}

/// Load a folder-based skill: reads `dir/SKILL.md`, uses directory name as skill id.
fn load_folder_skill(dir: &Path) -> Result<SkillDefinition, SkillError> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(SkillError::NotFound(format!(
            "SKILL.md not found in {}",
            dir.display()
        )));
    }
    let id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    parse_skill_file(&skill_md, &id)
}

/// Discover all skills in a directory.
///
/// Skills must follow the Claude Code folder format: `skills/{name}/SKILL.md`.
/// Directories starting with `_` are skipped (e.g. `_template`).
pub fn load_skills_dir(dir: impl AsRef<Path>) -> Result<Vec<SkillDefinition>, SkillError> {
    let dir = dir.as_ref();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut skills: Vec<SkillDefinition> = Vec::new();

    for entry in WalkDir::new(dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy();
        if dir_name.starts_with('_') {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        if skill_md.exists() {
            match load_folder_skill(entry.path()) {
                Ok(skill) => {
                    skills.push(skill);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %entry.path().display(),
                        %e,
                        "failed to load skill directory"
                    );
                }
            }
        }
    }

    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

/// Load skills from agent and project dirs; project skills override agent skills with same id.
pub fn load_skills_merged(
    project_skills_dir: impl AsRef<Path>,
    agent_skills_dir: impl AsRef<Path>,
) -> Result<Vec<SkillDefinition>, SkillError> {
    let mut by_id: std::collections::BTreeMap<String, SkillDefinition> =
        std::collections::BTreeMap::new();
    for skill in load_skills_dir(agent_skills_dir)? {
        by_id.insert(skill.id.clone(), skill);
    }
    for skill in load_skills_dir(project_skills_dir)? {
        by_id.insert(skill.id.clone(), skill);
    }
    Ok(by_id.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use tempfile::TempDir;

    fn create_skill(dir: &Path, id: &str, name: &str, description: &str, body: &str) {
        let skill_dir = dir.join(id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\nwhen_to_use: w\n---\n{body}"),
        )
        .unwrap();
    }

    // ── folder discovery tests ──

    #[rstest]
    #[test]
    fn load_folder_skill_basic() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills");
        create_skill(
            &skill_dir,
            "xianxia",
            "xianxia",
            "仙侠写作规范",
            "# 仙侠\n规则内容\n",
        );
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "xianxia");
        assert_eq!(skills[0].name, "xianxia");
        assert!(skills[0].body.contains("规则内容"));
    }

    #[rstest]
    #[test]
    fn load_skills_dir_multiple_skills() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills");
        create_skill(&skill_dir, "romance", "romance", "言情", "# 言情\n");
        create_skill(&skill_dir, "xianxia", "xianxia", "仙侠", "# 仙侠\n");
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert_eq!(skills.len(), 2);
    }

    #[rstest]
    #[test]
    fn load_skills_dir_skips_template_dir() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills");
        create_skill(&skill_dir, "_template", "template", "d", "body");
        create_skill(&skill_dir, "xianxia", "xianxia", "d", "body");
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "xianxia");
    }

    #[rstest]
    #[test]
    fn load_skills_dir_skips_non_skill_dirs() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills");
        std::fs::create_dir_all(skill_dir.join("random_folder")).unwrap();
        std::fs::write(skill_dir.join("random_folder/notes.md"), "not a skill").unwrap();
        create_skill(&skill_dir, "xianxia", "xianxia", "d", "body");
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "xianxia");
    }

    #[rstest]
    #[test]
    fn load_skills_dir_skips_underscore_dirs() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills");
        create_skill(&skill_dir, "_template", "t", "d", "body");
        create_skill(&skill_dir, "_private", "private", "d", "body");
        create_skill(&skill_dir, "xianxia", "xianxia", "d", "body");
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "xianxia");
    }

    #[rstest]
    #[test]
    fn load_skills_dir_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("nonexistent");
        let skills = load_skills_dir(&skill_dir).unwrap();
        assert!(skills.is_empty());
    }

    #[rstest]
    #[test]
    fn load_skill_direct_path() {
        let tmp = TempDir::new().unwrap();
        create_skill(
            tmp.path(),
            "xianxia",
            "xianxia",
            "仙侠",
            "# 仙侠\n规则内容\n",
        );
        let skill = load_skill(tmp.path().join("xianxia/SKILL.md")).unwrap();
        assert_eq!(skill.id, "xianxia");
        assert!(skill.body.contains("规则内容"));
    }

    #[rstest]
    #[test]
    fn load_skill_without_when_to_use() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("test");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: test\ndescription: desc\n---\nbody",
        )
        .unwrap();
        let skill = load_skill(skill_dir.join("SKILL.md")).unwrap();
        assert_eq!(skill.when_to_use, "");
    }

    // ── merged tests ──

    #[rstest]
    #[test]
    fn load_skills_merged_project_overrides_agent() {
        let tmp = TempDir::new().unwrap();
        let agent = tmp.path().join("agent_skills");
        let project = tmp.path().join("project_skills");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        create_skill(&agent, "xianxia", "xianxia", "agent", "agent body");
        create_skill(
            &agent,
            "romance",
            "romance",
            "agent romance",
            "agent romance body",
        );
        create_skill(&project, "xianxia", "xianxia", "project", "project body");

        let skills = load_skills_merged(&project, &agent).unwrap();
        assert_eq!(skills.len(), 2);
        let xianxia = skills.iter().find(|s| s.id == "xianxia").unwrap();
        assert!(xianxia.body.contains("project body"));
        let romance = skills.iter().find(|s| s.id == "romance").unwrap();
        assert!(romance.body.contains("agent romance body"));
    }

    #[rstest]
    #[test]
    fn load_skills_merged_folder_projects_both_dirs() {
        let tmp = TempDir::new().unwrap();
        let agent = tmp.path().join("agent_skills");
        let project = tmp.path().join("project_skills");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        create_skill(&agent, "mystery", "mystery", "agent", "agent mystery");
        create_skill(&project, "mystery", "mystery", "project", "project mystery");

        let skills = load_skills_merged(&project, &agent).unwrap();
        assert_eq!(skills.len(), 1);
        let mystery = skills.iter().find(|s| s.id == "mystery").unwrap();
        assert!(mystery.body.contains("project mystery"));
    }
}
