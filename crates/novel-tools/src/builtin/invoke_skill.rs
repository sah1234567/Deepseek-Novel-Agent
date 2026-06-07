use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_skills::load_skill;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct InvokeSkillTool;

/// Resolve a skill id to `skills/{skill_id}/SKILL.md`.
/// Checks project-level override first, then agent-level skills dir.
fn resolve_skill_path(
    project_root: &Path,
    skills_dir: Option<&Path>,
    skill_id: &str,
) -> Option<PathBuf> {
    // Project-level override
    let folder_path = project_root.join("skills").join(skill_id).join("SKILL.md");
    if folder_path.exists() {
        return Some(folder_path);
    }

    // Agent-level skills dir
    if let Some(dir) = skills_dir {
        let agent_path = dir.join(skill_id).join("SKILL.md");
        if agent_path.exists() {
            return Some(agent_path);
        }
    }

    None
}

#[async_trait]
impl Tool for InvokeSkillTool {
    fn name(&self) -> &str {
        "InvokeSkill"
    }

    fn description(&self) -> &str {
        "Load a genre skill body from skills/{id}/SKILL.md (folder format). \
         After loading, the SKILL.md body may reference additional files \
         (e.g. references/zombie.md) — use Read to open those on demand."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill_id": {
                    "type": "string",
                    "description": "Skill directory name under skills/"
                }
            },
            "required": ["skill_id"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let skill_id = require_str(&input, "skill_id")?;
        let path = resolve_skill_path(&ctx.project_root, ctx.skills_dir.as_deref(), &skill_id)
            .ok_or_else(|| {
                ToolError::Execution(format!("skill not found: skills/{skill_id}/SKILL.md"))
            })?;
        let skill = load_skill(&path).map_err(|e| ToolError::Execution(e.to_string()))?;

        // Build base directory prefix (like Claude Code's "Base directory for this skill")
        let base_dir = path
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| path.parent().unwrap_or(&path).to_path_buf());
        let base_dir_display = base_dir.display();
        let prefix = format!(
            "> Skill 根目录: {base_dir_display}/\n\
             > 引用文件请用此路径 + 相对路径拼接。如: {base_dir_display}/references/xxx.md\n\
             > 用 Read 工具和上述绝对路径读取引用文件。\n\n"
        );

        Ok(ToolOutput {
            content: prefix + &skill.body,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn loads_skill_body() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills").join("xianxia");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: xianxia\ndescription: d\nwhen_to_use: w\n---\n# 仙侠规则\n",
        )
        .unwrap();
        let tool = InvokeSkillTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"skill_id": "xianxia"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("Skill 根目录"));
        assert!(out.content.contains("仙侠规则"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn skill_not_found_returns_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let tool = InvokeSkillTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = tool
            .call(json!({"skill_id": "nonexistent"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
