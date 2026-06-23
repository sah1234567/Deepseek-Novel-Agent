//! Edit tool: string replacement with read-before-write enforcement.
//!
//! R1: single replace (`replace_all=false`) must target lines within committed spans.
//! `replace_all=true` bypasses R1 — only requires the file was Read/Tail'd this session
//! and the old_string exists on disk (byte-exact match).

use super::super::{
    blocking, extract_file_path, file_mtime_secs, require_str, EditCachePatch, Tool, ToolContext,
    ToolError, ToolOutput, ValidationError,
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
         Read/Tail output uses {line}\\t{content}; old_string/new_string must match on-disk bytes only \
         (no line-number prefix; copy from tool_result, not audit-report paraphrases). \
         Prefer Edit over Write for partial changes. old_string must be unique unless replace_all is true."
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

    fn can_write_outside_plan_dir(&self) -> bool {
        true
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        extract_file_path(input)?;
        require_str(input, "old_string")?;
        if input.get("new_string").is_none() {
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
        ctx.validate_plan_mode_write_path(self.name(), &path)?;
        ctx.require_read_before_write(self.name(), &full, &path, "editing", false)?;

        ctx.with_file_lock(&full, || async {
            let content = blocking::read_to_string(full.clone()).await?;
            crate::read_cache::verify_edit_against_read_cache(
                ctx.read_cache_entry(&full).as_ref(),
                &old_string,
                &content,
                &path,
                replace_all,
            )?;
            if let Some(entry) = ctx.read_cache_entry(&full) {
                let meta = tokio::fs::metadata(&full).await.map_err(ToolError::Io)?;
                entry.check_fresh_for_disk(file_mtime_secs(&meta), &content, "editing")?;
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

            blocking::write_for_rel_path(&path, full.clone(), updated.clone()).await?;

            let mtime = tokio::fs::metadata(&full)
                .await
                .map(|m| file_mtime_secs(&m))
                .unwrap_or(0);
            let occ = if replace_all { matches } else { 1 };
            ctx.patch_cache_after_edit(
                &full,
                &EditCachePatch {
                    updated_disk: &updated,
                    mtime_secs: mtime,
                    old_string: &old_string,
                    new_string: &new_string,
                    replace_all,
                    occurrences_replaced: occ,
                },
            );

            Ok(ToolOutput {
                content: format!("Edited {} ({occ} occurrence(s) replaced)", full.display()),
                is_error: false,
            })
        })
        .await
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
        ctx.file_op_locks = Some(Arc::new(dashmap::DashMap::new()));
        ctx
    }

    fn seed_committed_read_cache(
        ctx: &ToolContext,
        full: &std::path::Path,
        mut entry: ReadCacheEntry,
    ) {
        entry.commit_to_transcript();
        ctx.store_read_cache_direct(full, entry);
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
        seed_committed_read_cache(
            &ctx,
            &full,
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "foo bar foo".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
                transcript_committed: false,
                committed_spans: Vec::new(),
                committed_offset: None,
                committed_limit: None,
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
    async fn edit_allowed_when_target_line_in_partial_read_span() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        let body = (1..=50)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        write_file(tmp.path(), "chapters/ch01.md", &body);
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("chapters/ch01.md");
        let mtime = file_mtime_secs(&std::fs::metadata(&full).unwrap());
        seed_committed_read_cache(
            &ctx,
            &full,
            ReadCacheEntry {
                mtime_secs: mtime,
                raw_content: (33..=43)
                    .map(|n| format!("line {n}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                offset: Some(33),
                limit: Some(11),
                total_lines: 50,
                source: ReadCacheSource::Read,
                transcript_committed: false,
                committed_spans: Vec::new(),
                committed_offset: None,
                committed_limit: None,
            },
        );
        EditTool
            .call(
                json!({
                    "file_path": "chapters/ch01.md",
                    "old_string": "line 35",
                    "new_string": "line 35 edited"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let s = std::fs::read_to_string(tmp.path().join("chapters/ch01.md")).unwrap();
        assert!(s.contains("line 35 edited"));
        let entry = ctx.read_cache_entry(&full).unwrap();
        assert_eq!(entry.source, ReadCacheSource::EditPatched);
        assert_eq!(entry.offset, Some(33));
        assert_eq!(entry.total_lines, 50);
        assert_eq!(entry.limit, Some(11));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn partial_edit_expands_lines_in_cache() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        let body = (1..=30)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        write_file(tmp.path(), "chapters/ch01.md", &body);
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("chapters/ch01.md");
        let mtime = file_mtime_secs(&std::fs::metadata(&full).unwrap());
        seed_committed_read_cache(
            &ctx,
            &full,
            ReadCacheEntry {
                mtime_secs: mtime,
                raw_content: (10..=20)
                    .map(|n| format!("line {n}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                offset: Some(10),
                limit: Some(11),
                total_lines: 30,
                source: ReadCacheSource::Read,
                transcript_committed: false,
                committed_spans: Vec::new(),
                committed_offset: None,
                committed_limit: None,
            },
        );
        let replacement = "line 15 expanded\nline 15 extra";
        EditTool
            .call(
                json!({
                    "file_path": "chapters/ch01.md",
                    "old_string": "line 15",
                    "new_string": replacement
                }),
                &ctx,
            )
            .await
            .unwrap();
        let entry = ctx.read_cache_entry(&full).unwrap();
        assert_eq!(entry.total_lines, 31);
        assert_eq!(entry.limit, Some(12));
        assert!(entry.raw_content.contains("line 15 expanded"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replace_all_outside_partial_read_span_succeeds() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "a.md", "foo\nbar\nfoo");
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("a.md");
        let mtime = file_mtime_secs(&std::fs::metadata(&full).unwrap());
        seed_committed_read_cache(
            &ctx,
            &full,
            ReadCacheEntry {
                mtime_secs: mtime,
                raw_content: "bar".into(),
                offset: Some(2),
                limit: Some(1),
                total_lines: 3,
                source: ReadCacheSource::Read,
                transcript_committed: false,
                committed_spans: Vec::new(),
                committed_offset: None,
                committed_limit: None,
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
        assert_eq!(s, "baz\nbar\nbaz");
        let entry = ctx.read_cache_entry(&full).unwrap();
        assert!(entry.is_full_read());
        assert_eq!(entry.raw_content, s);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multi_match_without_replace_all_errors() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "a.md", "x x");
        let ctx = ctx_with_cache(&tmp);
        let full = ctx.resolve_path("a.md");
        seed_committed_read_cache(
            &ctx,
            &full,
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "x x".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
                transcript_committed: false,
                committed_spans: Vec::new(),
                committed_offset: None,
                committed_limit: None,
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
