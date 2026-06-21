use crate::frontmatter::parse_frontmatter;
use crate::memory_scan::scan_memory_files;
use crate::memory_types::{
    memory_type_description, truncate_bytes_utf8, MemoryFrontmatter, MEMORY_TYPES,
};
use std::path::Path;

/// Load memory/ files into a Markdown block for the system prompt.
///
/// Scans `memory/` subdirectories (style/, plot_decisions/,
/// character_guardrails/, feedback/, references/), reads frontmatter via
/// [`parse_frontmatter`], filters out inactive files via
/// [`MemoryStatus::is_active`], then loads body content ≤ `max_bytes`.
/// Type is derived from the directory — flat root files are ignored.
pub fn load_memory(project_root: &Path, max_bytes: usize) -> String {
    let memory_dir = project_root.join("memory");

    let headers = scan_memory_files(&memory_dir);

    if headers.is_empty() {
        return String::new();
    }

    // Build output: headers section + body content
    let mut out = String::new();

    // Add a brief header with type descriptions
    out.push_str("## 作品记忆\n\n");
    out.push_str("| Type | 存储内容 |\n");
    out.push_str("|------|---------|\n");
    for &type_name in MEMORY_TYPES {
        let desc = memory_type_description(type_name);
        out.push_str(&format!("| {type_name} | {desc} |\n"));
    }
    out.push('\n');

    for header in &headers {
        let body = read_memory_body(project_root, &header.rel_path, max_bytes);
        if body.is_empty() {
            continue;
        }
        let entry = format_memory_entry(header, &body);
        if out.len() + entry.len() >= max_bytes {
            out.push_str(&format!(
                "\n> WARNING: memory content truncated ({} bytes limit). Only partial memory loaded.\n",
                max_bytes
            ));
            // Add what we can fit
            let remaining = max_bytes.saturating_sub(out.len());
            if remaining > 100 {
                out.push_str(&truncate_str(&entry, remaining));
            }
            break;
        }
        out.push_str(&entry);
    }

    if out.len() > max_bytes {
        truncate_str(&out, max_bytes)
    } else {
        out
    }
}

fn read_memory_body(project_root: &Path, rel_path: &str, max_body_bytes: usize) -> String {
    let path = project_root.join("memory").join(rel_path);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    // Parse frontmatter to extract body
    if let Ok((_fm, body)) = parse_frontmatter::<MemoryFrontmatter>(&content) {
        if body.len() <= max_body_bytes {
            body
        } else {
            truncate_bytes_utf8(&body, max_body_bytes)
        }
    } else {
        // No valid frontmatter — return raw content as fallback
        if content.len() <= max_body_bytes {
            content
        } else {
            truncate_bytes_utf8(&content, max_body_bytes)
        }
    }
}

fn format_memory_entry(header: &crate::memory_types::MemoryHeader, body: &str) -> String {
    let type_label = header.memory_type.label();
    let fm = &header.frontmatter;
    let mut entry = format!("\n### [{type_label}] {} ({})\n", fm.name, fm.chapter);
    entry.push_str(&format!("_{}_\n\n", fm.description));
    entry.push_str(body);
    entry.push('\n');
    entry
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!(
            "{}\n> WARNING: content was truncated ({} → {} bytes). Only part of it was loaded.",
            truncate_bytes_utf8(s, max),
            s.len(),
            max
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn truncate_str_does_not_split_utf8_codepoint() {
        let s = "记".repeat(100);
        let out = truncate_str(&s, 80);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.contains('…'));
    }

    #[test]
    fn load_memory_empty_when_missing() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        assert!(load_memory(tmp.path(), 4096).is_empty());
    }

    #[test]
    fn load_memory_reads_memory_in_subdir() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let mem = tmp.path().join("memory");
        let guard_dir = mem.join("character_guardrails");
        fs::create_dir_all(&guard_dir).expect("dir");
        let content = "---\nname: hero\ndescription: 主角设定\nchapter: Ch1\nstatus: active\n---\n\n陈默永远不能背叛伙伴。\n\n**Why:** 核心人设\n**How to apply:** 所有场景";
        fs::write(guard_dir.join("hero.md"), content).expect("w");
        let out = load_memory(tmp.path(), 4096);
        assert!(out.contains("hero"), "should contain memory name: {out}");
        assert!(
            out.contains("陈默永远不能背叛伙伴"),
            "should contain body: {out}"
        );
    }

    #[test]
    fn load_memory_reads_typed_subdirs() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).expect("dir");
        let content = "---\nname: pacing\ndescription: 节奏偏好\nchapter: Ch1\nstatus: active\n---\n\n每章结尾必须有悬念或冲突转折。\n\n**Why:** 保持读者粘性\n**How to apply:** 每章写作时检查";
        fs::write(style_dir.join("pacing.md"), content).expect("w");
        let out = load_memory(tmp.path(), 4096);
        assert!(out.contains("pacing"), "should contain memory name: {out}");
        assert!(
            out.contains("每章结尾必须有悬念"),
            "should contain body: {out}"
        );
    }

    #[test]
    fn load_memory_skips_deprecated() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).expect("dir");

        let active_content = "---\nname: active-rule\ndescription: 保持规则\nchapter: Ch1\nstatus: active\n---\n\nActive rule body.\n\n**Why:** test\n**How to apply:** test";
        fs::write(style_dir.join("active-rule.md"), active_content).expect("w");

        let deprecated_content = "---\nname: old-rule\ndescription: 旧规则\nchapter: Ch1\nstatus: deprecated\n---\n\nOld rule that should not appear.\n\n**Why:** outdated\n**How to apply:** n/a";
        fs::write(style_dir.join("old-rule.md"), deprecated_content).expect("w");

        let out = load_memory(tmp.path(), 4096);
        assert!(out.contains("active-rule"), "should contain active: {out}");
        assert!(
            !out.contains("old-rule"),
            "should NOT contain deprecated: {out}"
        );
    }

    #[test]
    fn load_memory_skips_templates() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).expect("dir");
        fs::write(style_dir.join("_template.md"), "template content").expect("w");
        let out = load_memory(tmp.path(), 4096);
        assert!(!out.contains("template"), "should skip templates: {out}");
    }

    #[test]
    fn scan_memory_headers_sorts_by_mtime() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let style_dir = tmp.path().join("memory").join("style");
        fs::create_dir_all(&style_dir).expect("dir");

        let content1 =
            "---\nname: first\ndescription: d\nchapter: Ch1\nstatus: active\n---\n\nBody 1";
        let content2 =
            "---\nname: second\ndescription: d\nchapter: Ch2\nstatus: active\n---\n\nBody 2";

        fs::write(style_dir.join("first.md"), content1).expect("w");
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(style_dir.join("second.md"), content2).expect("w");

        let out = load_memory(tmp.path(), 4096);
        // second (newer) should come before first
        let pos_second = out.find("second").unwrap_or(usize::MAX);
        let pos_first = out.find("first").unwrap_or(usize::MAX);
        assert!(
            pos_second < pos_first,
            "newer file should appear first; second at {pos_second}, first at {pos_first}"
        );
    }

    #[test]
    fn load_memory_truncates_at_limit() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let style_dir = tmp.path().join("memory").join("style");
        fs::create_dir_all(&style_dir).expect("dir");

        let long_body = "长".repeat(5000);
        let content = format!(
            "---\nname: long\ndescription: d\nchapter: Ch1\nstatus: active\n---\n\n{long_body}"
        );
        fs::write(style_dir.join("long.md"), content).expect("w");
        let out = load_memory(tmp.path(), 512);
        // Should be truncated and contain the warning
        assert!(out.len() <= 600, "truncated output should be ≤~600 bytes");
    }
}
