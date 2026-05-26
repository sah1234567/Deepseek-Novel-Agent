use super::super::{
    blocking, normalize_rel_path, optional_search_root, optional_str_any, Tool, ToolContext,
    ToolError, ToolOutput,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct GlobTool;

/// Agent-friendly glob: trailing `/*` searches the whole subtree (`/**`).
fn expand_agent_glob_pattern(pattern: &str) -> String {
    let p = normalize_rel_path(pattern);
    if p.ends_with("/*") && !p.ends_with("/**") {
        format!("{}/**", &p[..p.len() - 2])
    } else {
        p
    }
}

/// Basename-only patterns match anywhere under the walk root (`**/*.md`).
fn effective_match_patterns(pattern: &str, search_rel_from_project: &str) -> Vec<String> {
    let expanded = expand_agent_glob_pattern(pattern);
    let mut out = Vec::new();
    if expanded.contains('/') {
        out.push(expanded.clone());
    } else {
        out.push(format!("**/{}", expanded));
    }
    if search_rel_from_project != "." && !search_rel_from_project.is_empty() {
        let prefixed = if expanded.contains('/') {
            expanded.clone()
        } else {
            format!("**/{}", expanded)
        };
        let combined = format!("{}/{}", search_rel_from_project, prefixed);
        if !out.iter().any(|p| p == &combined) {
            out.push(combined);
        }
    }
    out
}

/// Strip absolute project-root prefix from a pattern if present.
fn strip_project_prefix(pattern: &str, project_root: &Path) -> String {
    let norm = normalize_rel_path(pattern);
    let root_norm = project_root
        .canonicalize()
        .map(|p| normalize_rel_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| normalize_rel_path(&project_root.to_string_lossy()));
    if norm.starts_with(&root_norm) {
        let rest = norm[root_norm.len()..].trim_start_matches('/');
        if rest.is_empty() {
            "**".to_string()
        } else {
            rest.to_string()
        }
    } else {
        norm
    }
}

fn rel_path_from_root(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn search_rel_from_project(search_root: &Path, project_root: &Path) -> String {
    let search = search_root
        .canonicalize()
        .unwrap_or_else(|_| search_root.to_path_buf());
    let project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    rel_path_from_root(&search, &project)
}

/// Single path segment: `*` and `?` only (no `/`).
fn str_glob_match(text: &str, pat: &str) -> bool {
    let t = text.as_bytes();
    let p = pat.as_bytes();
    let (mut ti, mut pi) = (0, 0);
    let (mut star_t, mut star_p) = (None::<usize>, None::<usize>);

    while ti < t.len() || pi < p.len() {
        if pi < p.len() && (p[pi] == b'?' || (p[pi] != b'*' && ti < t.len() && t[ti] == p[pi])) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_p = Some(pi);
            star_t = Some(ti);
            pi += 1;
        } else if let (Some(st), Some(sp)) = (star_t, star_p) {
            if st >= t.len() {
                return false;
            }
            pi = sp + 1;
            star_t = Some(st + 1);
            ti = st + 1;
        } else {
            return false;
        }
    }
    true
}

fn glob_path_match(path: &str, pattern: &str) -> bool {
    let path = normalize_rel_path(path);
    let pattern = normalize_rel_path(pattern);
    glob_segments_match(
        &path.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
        &pattern
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
    )
}

fn glob_segments_match(path: &[&str], pat: &[&str]) -> bool {
    match (path, pat) {
        ([], []) => true,
        (_, []) => false,
        ([], [head, tail @ ..]) if *head == "**" => glob_segments_match(&[], tail),
        (path, [head, tail @ ..]) if *head == "**" => (0..=path.len())
            .any(|i| glob_segments_match(&path[i..], tail)),
        ([p, ps @ ..], [q, qs @ ..]) if str_glob_match(p, q) => glob_segments_match(ps, qs),
        _ => false,
    }
}

fn glob_sync(
    walk_root: PathBuf,
    project_root: PathBuf,
    pattern: String,
) -> Result<ToolOutput, ToolError> {
    let pattern = strip_project_prefix(&pattern, &project_root);
    let search_rel = search_rel_from_project(&walk_root, &project_root);
    let match_patterns = effective_match_patterns(&pattern, &search_rel);

    let mut files = Vec::new();
    for entry in WalkDir::new(&walk_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = rel_path_from_root(entry.path(), &project_root);
        if match_patterns.iter().any(|pat| glob_path_match(&rel, pat)) {
            files.push(rel);
        }
    }
    files.sort();
    files.dedup();
    Ok(ToolOutput {
        content: files.join("\n"),
        is_error: false,
    })
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern (supports path prefixes, *, **, ?; patterns without / match any depth)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "search_root": {
                    "type": "string",
                    "description": "Directory to search under project root (default: project root)"
                }
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = optional_str_any(&input, &["pattern", "glob_pattern", "glob"])
            .unwrap_or_else(|| "*".into());
        let walk_root = optional_search_root(&input)
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| ctx.project_root.clone());
        let project_root = ctx.project_root.clone();
        blocking::run_blocking(move || glob_sync(walk_root, project_root, pattern)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_under_project;
    use std::fs;
    use tempfile::TempDir;

    fn write_tree(root: &Path) {
        fs::create_dir_all(root.join("knowledge/plot/细纲")).unwrap();
        fs::create_dir_all(root.join("chapters")).unwrap();
        fs::write(
            root.join("knowledge/plot/细纲/chapter-001-细纲.md"),
            "# outline",
        )
        .unwrap();
        fs::write(root.join("knowledge/INDEX.md"), "# index").unwrap();
        fs::write(root.join("chapters/chapter-001.md"), "# ch1").unwrap();
        fs::write(root.join("chapters/chapter-002.md"), "# ch2").unwrap();
        fs::write(root.join("readme.txt"), "hi").unwrap();
    }

    fn glob_lines(root: &Path, pattern: &str) -> Vec<String> {
        let project_root = root.to_path_buf();
        let out = glob_sync(project_root.clone(), project_root, pattern.to_string()).unwrap();
        out.content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn glob_with_root(root: &Path, walk: &str, pattern: &str) -> Vec<String> {
        let project_root = root.to_path_buf();
        let walk_root = resolve_under_project(&project_root, walk);
        let out = glob_sync(walk_root, project_root, pattern.to_string()).unwrap();
        out.content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn glob_path_match_basics() {
        assert!(glob_path_match(
            "knowledge/plot/细纲/chapter-001-细纲.md",
            "knowledge/**"
        ));
        assert!(glob_path_match("chapters/chapter-001.md", "chapters/chapter-*.md"));
        assert!(!glob_path_match("chapters/chapter-001.md", "knowledge/*"));
        assert!(glob_path_match(
            "knowledge/plot/细纲/chapter-001-细纲.md",
            "**/*.md"
        ));
        assert!(glob_path_match("readme.txt", "*.txt"));
    }

    #[test]
    fn expand_trailing_slash_star() {
        assert_eq!(
            expand_agent_glob_pattern("knowledge/*"),
            "knowledge/**"
        );
    }

    #[test]
    fn strip_project_prefix_windows_style() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let pattern = format!(
            "{}/knowledge/*.md",
            normalize_rel_path(&root.to_string_lossy())
        );
        assert_eq!(
            strip_project_prefix(&pattern, &root),
            "knowledge/*.md"
        );
    }

    #[test]
    fn pattern_without_dir_recursive_md() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "*.md");
        assert!(hits.iter().any(|p| p.contains("细纲")));
        assert!(hits.iter().any(|p| p == "chapters/chapter-001.md"));
        assert!(!hits.iter().any(|p| p.ends_with("readme.txt")));
    }

    #[test]
    fn pattern_with_dir_prefix_knowledge() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "knowledge/*");
        assert!(hits.iter().any(|p| p.contains("细纲/chapter-001-细纲.md")));
        assert!(hits.iter().any(|p| p == "knowledge/INDEX.md"));
        assert!(!hits.iter().any(|p| p.starts_with("chapters/")));
    }

    #[test]
    fn pattern_chapters_prefix() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "chapters/*");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| p.starts_with("chapters/chapter-")));
    }

    #[test]
    fn pattern_chapters_chapter_glob() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "chapters/chapter-*.md");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn search_root_with_basename_pattern() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_with_root(tmp.path(), "chapters", "chapter-*.md");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| p.starts_with("chapters/")));
    }

    #[test]
    fn search_root_dot_with_nested_chinese() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_with_root(tmp.path(), ".", "knowledge/plot/细纲/*.md");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].contains("chapter-001-细纲.md"));
    }

    #[test]
    fn backslash_pattern_normalized() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "chapters\\chapter-001.md");
        assert_eq!(hits, vec!["chapters/chapter-001.md"]);
    }

    #[test]
    fn dot_slash_prefix() {
        let tmp = TempDir::new().unwrap();
        write_tree(tmp.path());
        let hits = glob_lines(tmp.path(), "./knowledge/INDEX.md");
        assert_eq!(hits, vec!["knowledge/INDEX.md"]);
    }
}
