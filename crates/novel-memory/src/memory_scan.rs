//! Memory file scanning: walk `memory/` directory, read frontmatter from
//! each `.md` file, and produce a manifest for LLM-based relevance selection.

use crate::frontmatter::parse_frontmatter;
use crate::memory_types::{MemoryConstants, MemoryFrontmatter, MemoryHeader, MemoryType};
use std::fs;
use std::path::Path;

/// Scan active memory files (excludes deprecated/superseded) for prefetch/selector.
pub fn scan_memory_files(memory_dir: &Path) -> Vec<MemoryHeader> {
    scan_memory_files_inner(memory_dir, false)
}

/// Scan all memory files including deprecated/superseded (for extractMemories subagent).
pub fn scan_memory_files_for_extraction(memory_dir: &Path) -> Vec<MemoryHeader> {
    scan_memory_files_inner(memory_dir, true)
}

fn scan_memory_files_inner(memory_dir: &Path, include_deprecated: bool) -> Vec<MemoryHeader> {
    let mut headers: Vec<MemoryHeader> = Vec::new();
    if !memory_dir.is_dir() {
        return headers;
    }
    walk_memory_dir(memory_dir, &mut |path, rel| {
        if let Some(h) = try_read_memory_header(path, rel, include_deprecated) {
            headers.push(h);
        }
    });
    headers.sort_by_key(|h| std::cmp::Reverse(h.mtime_ms));
    headers.truncate(MemoryConstants::MAX_MEMORY_FILES);
    headers
}

/// Walk memory/ subdirectories and call `on_file` for each .md file found.
/// Only subdirectories are scanned — flat files in the root are ignored
/// (type is derived from the directory name).
fn walk_memory_dir(memory_dir: &Path, on_file: &mut dyn FnMut(&Path, &str)) {
    let Ok(entries) = fs::read_dir(memory_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
        walk_memory_subdir(&path, &dir_name, on_file);
    }
}

fn walk_memory_subdir(dir: &Path, dir_name: &str, on_file: &mut dyn FnMut(&Path, &str)) {
    let Ok(sub_entries) = fs::read_dir(dir) else {
        return;
    };
    for sub_entry in sub_entries.flatten() {
        let sub_path = sub_entry.path();
        if !is_non_template_md(&sub_path) {
            continue;
        }
        let rel = format!(
            "{}/{}",
            dir_name,
            sub_path.file_name().unwrap_or_default().to_string_lossy()
        );
        on_file(&sub_path, &rel);
    }
}

fn is_non_template_md(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md") && path.file_stem().is_none_or(|s| s != "_template")
}

/// Try to parse a single memory file into a [`MemoryHeader`].
/// Returns `None` on any failure (unreadable file, invalid frontmatter,
/// or excluded by the deprecated/superseded filter).
fn try_read_memory_header(
    path: &Path,
    rel: &str,
    include_deprecated: bool,
) -> Option<MemoryHeader> {
    let content = read_file_head(path, MemoryConstants::FRONTMATTER_MAX_LINES).ok()?;
    let (fm, _body): (MemoryFrontmatter, _) = parse_frontmatter(&content).ok()?;

    if !include_deprecated && !fm.status.is_active() {
        return None;
    }

    // Derive memory type from directory: "style/pacing.md" → Style
    let memory_type = MemoryType::from_rel_path(rel)?;

    let mtime_ms = fs::metadata(path)
        .ok()
        .and_then(|meta| {
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
        })
        .unwrap_or(0);

    Some(MemoryHeader {
        rel_path: rel.to_string(),
        memory_type,
        frontmatter: fm,
        mtime_ms,
    })
}

/// Read the first `max_lines` lines of a file (for frontmatter scanning).
/// This avoids reading the entire file body when we only need the YAML header.
fn read_file_head(path: &Path, max_lines: usize) -> Result<String, std::io::Error> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().take(max_lines).collect();
    Ok(lines.join("\n"))
}

/// Format a list of `MemoryHeader`s into a manifest text for LLM selection.
///
/// Each line has the format: `[{type}] {filename}: {description}`
///
/// When `include_deprecated` is true, deprecated files are included with
/// a `[DEPRECATED]` prefix (used by extractMemories subagent).
/// When false (selector path), deprecated files should already be filtered out.
///
/// Example output (selector):
/// ```text
/// [plot_decision] cp-pairings.md: 陈默与林若烟为最终CP，不设感情三角
/// [style] pacing.md: 每章结尾必须有悬念或冲突转折
/// ```
pub fn format_memory_manifest(headers: &[MemoryHeader], include_deprecated: bool) -> String {
    let mut manifest = String::new();
    for h in headers {
        let type_str = h.memory_type.as_str();
        let deprecated_prefix = if include_deprecated && !h.frontmatter.status.is_active() {
            "[DEPRECATED] "
        } else {
            ""
        };
        let line = format!(
            "{deprecated_prefix}[{type_str}] {}: {}\n",
            h.rel_path, h.frontmatter.description
        );
        manifest.push_str(&line);
    }
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_types::MemoryStatus;
    use std::fs;
    use tempfile::TempDir;

    fn write_memory(dir: &Path, rel: &str, name: &str, desc: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let content = format!(
            "---\nname: {name}\ndescription: {desc}\nchapter: Ch1\nstatus: active\n---\n\nBody for {name}",
        );
        fs::write(&path, content).unwrap();
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let headers = scan_memory_files(tmp.path());
        assert!(headers.is_empty());
    }

    #[test]
    fn scan_finds_files_in_subdirs() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memory");
        write_memory(&mem, "style/pacing.md", "pacing", "节奏偏好");
        write_memory(&mem, "plot_decisions/cp.md", "cp-pairing", "CP设定");

        let headers = scan_memory_files(&mem);
        assert_eq!(headers.len(), 2);
        assert!(headers.iter().any(|h| h.rel_path.contains("pacing")));
        assert!(headers.iter().any(|h| h.rel_path.contains("cp")));
    }

    #[test]
    fn scan_includes_deprecated_for_extraction() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).unwrap();

        let active =
            "---\nname: active\ndescription: d\nchapter: Ch1\nstatus: active\n---\n\nActive";
        let deprecated =
            "---\nname: old\ndescription: d\nchapter: Ch1\nstatus: deprecated\n---\n\nOld";

        fs::write(style_dir.join("active.md"), active).unwrap();
        fs::write(style_dir.join("old.md"), deprecated).unwrap();

        let selector_headers = scan_memory_files(&mem);
        assert_eq!(selector_headers.len(), 1);

        let extract_headers = scan_memory_files_for_extraction(&mem);
        assert_eq!(extract_headers.len(), 2);
        let manifest = format_memory_manifest(&extract_headers, true);
        assert!(manifest.contains("[DEPRECATED]"));
    }

    #[test]
    fn scan_skips_deprecated() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).unwrap();

        let active =
            "---\nname: active\ndescription: d\nchapter: Ch1\nstatus: active\n---\n\nActive";
        let deprecated =
            "---\nname: old\ndescription: d\nchapter: Ch1\nstatus: deprecated\n---\n\nOld";

        fs::write(style_dir.join("active.md"), active).unwrap();
        fs::write(style_dir.join("old.md"), deprecated).unwrap();

        let headers = scan_memory_files(&mem);
        assert_eq!(headers.len(), 1);
        assert!(headers[0].rel_path.contains("active"));
    }

    #[test]
    fn scan_skips_templates() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).unwrap();
        fs::write(style_dir.join("_template.md"), "template").unwrap();

        let headers = scan_memory_files(&mem);
        assert!(headers.is_empty());
    }

    #[test]
    fn format_manifest_produces_expected_output() {
        let headers = vec![MemoryHeader {
            rel_path: "style/pacing.md".into(),
            memory_type: MemoryType::Style,
            frontmatter: MemoryFrontmatter {
                name: "pacing".into(),
                description: "节奏偏好".into(),
                chapter: "Ch1".into(),
                status: MemoryStatus::Active,
            },
            mtime_ms: 1000,
        }];

        let manifest = format_memory_manifest(&headers, false);
        assert!(manifest.contains("[style]"));
        assert!(manifest.contains("pacing.md"));
        assert!(manifest.contains("节奏偏好"));
    }

    #[test]
    fn scan_sorts_by_mtime_newest_first() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memory");
        let style_dir = mem.join("style");
        fs::create_dir_all(&style_dir).unwrap();

        write_memory(&mem, "style/first.md", "first", "d");
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_memory(&mem, "style/second.md", "second", "d");

        let headers = scan_memory_files(&mem);
        assert_eq!(headers.len(), 2);
        // second (newer) should be first
        assert!(headers[0].rel_path.contains("second"));
        assert!(headers[1].rel_path.contains("first"));
    }
}
