use std::path::Path;

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

pub(super) fn truncate_str(s: &str, max: usize) -> String {
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
