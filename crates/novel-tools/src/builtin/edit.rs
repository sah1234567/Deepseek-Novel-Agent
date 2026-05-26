use super::super::{
    blocking, extract_file_path, file_mtime_secs, require_str, Tool, ToolContext, ToolError,
    ToolOutput, ValidationError,
};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct EditTool;

fn old_string_has_line_prefix(s: &str) -> bool {
    s.lines().any(|line| {
        if let Some((prefix, rest)) = line.split_once('\t') {
            !rest.is_empty() && !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit())
        } else {
            false
        }
    })
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "Exact string replacement in an existing file. Must Read or Tail the file first. \
         Read/Tail output uses {line}\\t{content}; old_string/new_string must match file bytes only \
         (no line-number prefix). Prefer Edit over Write for partial changes. \
         old_string must be unique unless replace_all is true."
    }

    fn usage_hint(&self) -> &str {
        "Append log row: old_string=table last row, new_string=last row + new line. Multi-match: replace_all or longer old_string."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Relative path under project root"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace (no Read line-number prefix)"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text (must differ from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence (default false). If false, old_string must be unique."
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        extract_file_path(input)?;
        require_str(input, "old_string")?;
        if !input.get("new_string").is_some() {
            return Err(ValidationError::MissingField("new_string".into()));
        }
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = extract_file_path(&input)?;
        let old_string = require_str(&input, "old_string")?.to_string();
        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if old_string == new_string {
            return Err(ToolError::Execution(
                "No changes: old_string and new_string are identical.".into(),
            ));
        }

        if old_string_has_line_prefix(&old_string) {
            return Err(ToolError::Execution(
                "old_string appears to contain Read/Tail line-number prefix (digits + tab). \
                 Match raw file text only — content after the tab on each line."
                    .into(),
            ));
        }

        let full = ctx.resolve_path(&path);
        ctx.validate_write_root(&full)?;
        ctx.validate_plan_mode_write_path(&path)?;
        ctx.require_read_before_write(&full, &path, "editing", false)?;
        ctx.require_edit_in_read_slice(&full, &old_string)?;

        let content = blocking::read_to_string(full.clone()).await?;
        if let Some(entry) = ctx.read_cache_entry(&full) {
            let meta = tokio::fs::metadata(&full).await.map_err(ToolError::Io)?;
            entry.check_fresh_for_disk(file_mtime_secs(&meta), &content, "editing")?;
        }

        if !content.contains(&old_string) {
            let preview: String = old_string.chars().take(80).collect();
            return Err(ToolError::Execution(format!(
                "old_string not found in {}.\nPreview: {preview}",
                full.display()
            )));
        }

        let matches = content.matches(&old_string).count();
        if matches > 1 && !replace_all {
            return Err(ToolError::Execution(format!(
                "Found {matches} matches of old_string, but replace_all is false. \
                 Set replace_all to true, or provide more context so old_string is unique."
            )));
        }

        let updated = if replace_all {
            content.replace(&old_string, &new_string)
        } else {
            content.replacen(&old_string, &new_string, 1)
        };

        blocking::write(full.clone(), updated.clone()).await?;

        let mtime = tokio::fs::metadata(&full)
            .await
            .map(|m| file_mtime_secs(&m))
            .unwrap_or(0);
        ctx.refresh_cache_after_write(&full, &updated, mtime);

        let occ = if replace_all { matches } else { 1 };
        Ok(ToolOutput {
            content: format!("Edited {} ({occ} occurrence(s) replaced)", full.display()),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReadCacheEntry, ReadCacheSource};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn ctx_with_cache(tmp: &TempDir) -> ToolContext {
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        ctx.read_file_cache = Some(Arc::new(dashmap::DashMap::new()));
        ctx
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replace_all_replaces_every_match() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "a.md", "foo bar foo");
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("a.md");
        ctx.store_read_cache(
            &full,
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "foo bar foo".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
            },
        );
        EditTool
            .call(
                json!({
                    "file_path": "a.md",
                    "old_string": "foo",
                    "new_string": "baz",
                    "replace_all": true
                }),
                &ctx,
            )
            .await
            .unwrap();
        let s = std::fs::read_to_string(tmp.path().join("a.md")).unwrap();
        assert_eq!(s, "baz bar baz");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multi_match_without_replace_all_errors() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "a.md", "x x");
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("a.md");
        ctx.store_read_cache(
            &full,
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "x x".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
            },
        );
        let err = EditTool
            .call(
                json!({"file_path": "a.md", "old_string": "x", "new_string": "y"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("replace_all"));
    }
}
