//! Render spec_template / path_template with generic cursor counters and tags.
//!
//! Supported syntax:
//! - `{{cursor.X}}`        → counter or tag value
//! - `{{cursor.X | padN}}` → zero-padded counter (e.g. pad3 → 007)
//! - `{{cursor.X | sub N}}`→ counter minus N
//! - `{{custom_key}}`       → from extras HashMap

use crate::types::Cursor;
use std::collections::HashMap;

pub fn render_template(
    template: &str,
    cursor: &Cursor,
    extras: &HashMap<String, String>,
) -> String {
    let mut out = template.to_string();

    // Find and process all {{cursor...}} expressions, longest first to avoid
    // partial overlaps (e.g. {{cursor.s}} inside {{cursor.section}}).
    let mut exprs = find_all_cursor_exprs(&out);
    exprs.sort_by_key(|b| std::cmp::Reverse(b.raw.len()));

    for expr in &exprs {
        let replacement = evaluate_cursor_expr(expr, cursor);
        out = out.replace(&expr.raw, &replacement);
    }

    // Extras {{key}}
    for (k, v) in extras {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }

    out
}

struct CursorExpr {
    raw: String,
    key: String,
    filter: Option<String>,
    filter_arg: String,
}

/// Find ALL `{{cursor...}}` patterns (with or without filters).
fn find_all_cursor_exprs(template: &str) -> Vec<CursorExpr> {
    let mut results = Vec::new();
    let mut i = 0;
    while let Some(start) = template[i..].find("{{cursor.") {
        let abs_start = i + start;
        let content_start = abs_start + 9; // after "{{cursor."
                                           // Find matching "}}" — use the first occurrence
        if let Some(end) = template[content_start..].find("}}") {
            let abs_end = content_start + end;
            let raw = template[abs_start..abs_end + 2].to_string();
            let inner = template[content_start..abs_end].trim();

            let (key, filter, filter_arg) = if let Some(pipe_pos) = inner.find('|') {
                let k = inner[..pipe_pos].trim().to_string();
                let fp = inner[pipe_pos + 1..].trim();
                let (f, a) = parse_filter(fp);
                (k, f, a)
            } else {
                (inner.to_string(), None, String::new())
            };

            results.push(CursorExpr {
                raw,
                key,
                filter,
                filter_arg,
            });
            i = abs_end + 2;
        } else {
            break;
        }
    }
    results
}

/// Parse "pad3" → ("pad", "3"), "sub 2" → ("sub", "2"), "pad" → ("pad", "")
fn parse_filter(filter_part: &str) -> (Option<String>, String) {
    let trimmed = filter_part.trim();
    if trimmed.is_empty() {
        return (None, String::new());
    }
    // Space-separated: "sub 2"
    if let Some(space_pos) = trimmed.find(' ') {
        let filter = trimmed[..space_pos].trim().to_string();
        let arg = trimmed[space_pos + 1..].trim().to_string();
        return (Some(filter), arg);
    }
    // Numeric suffix: "pad3", "sub2"
    if let Some(digit_pos) = trimmed.find(|c: char| c.is_ascii_digit()) {
        if digit_pos > 0 {
            let filter = trimmed[..digit_pos].to_string();
            let arg = trimmed[digit_pos..].to_string();
            return (Some(filter), arg);
        }
    }
    (Some(trimmed.to_string()), String::new())
}

/// Evaluate a cursor expression to its replacement string.
fn evaluate_cursor_expr(expr: &CursorExpr, cursor: &Cursor) -> String {
    match expr.filter.as_deref() {
        Some("pad") => {
            let width: usize = expr.filter_arg.parse().unwrap_or(3);
            let val = cursor.counters.get(&expr.key).copied().unwrap_or(0);
            format!("{:0width$}", val, width = width)
        }
        Some("sub") => {
            let n: i64 = expr.filter_arg.parse().unwrap_or(0);
            let val = cursor.counters.get(&expr.key).copied().unwrap_or(0);
            format!("{}", val.saturating_sub(n))
        }
        None => {
            // Plain {{cursor.X}} — no filter
            if let Some(v) = cursor.counters.get(&expr.key) {
                return v.to_string();
            }
            if let Some(v) = cursor.tags.get(&expr.key) {
                return v.clone();
            }
            // Keep original placeholder for unknown keys
            expr.raw.clone()
        }
        _ => expr.raw.clone(), // unknown filter — leave as-is
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_with(chapter: i64, volume: i64, round: i64) -> Cursor {
        let mut counters = HashMap::new();
        counters.insert("chapter".into(), chapter);
        counters.insert("volume".into(), volume);
        counters.insert("round".into(), round);
        Cursor {
            counters,
            tags: HashMap::new(),
        }
    }

    #[test]
    fn renders_chapter_pad() {
        let c = cursor_with(7, 1, 7);
        let s = render_template(
            "chapters/chapter-{{cursor.chapter | pad3}}.md",
            &c,
            &HashMap::new(),
        );
        assert_eq!(s, "chapters/chapter-007.md");
    }

    #[test]
    fn renders_simple_counter() {
        let c = cursor_with(3, 2, 1);
        let s = render_template(
            "Write chapter {{cursor.chapter}}, volume {{cursor.volume}}",
            &c,
            &HashMap::new(),
        );
        assert_eq!(s, "Write chapter 3, volume 2");
    }

    #[test]
    fn renders_sub_filter() {
        let c = cursor_with(5, 1, 1);
        let s = render_template(
            "Chapters {{cursor.chapter | sub 2}}–{{cursor.chapter}}",
            &c,
            &HashMap::new(),
        );
        assert_eq!(s, "Chapters 3–5");
    }

    #[test]
    fn renders_extras() {
        let c = cursor_with(1, 1, 1);
        let mut extras = HashMap::new();
        extras.insert("foo".into(), "bar".into());
        let s = render_template("{{foo}} ch{{cursor.chapter}}", &c, &extras);
        assert_eq!(s, "bar ch1");
    }

    #[test]
    fn missing_counter_keeps_placeholder() {
        let c = Cursor::default();
        let s = render_template("{{cursor.nonexistent}}", &c, &HashMap::new());
        assert_eq!(s, "{{cursor.nonexistent}}");
    }

    #[test]
    fn generic_counter_names() {
        let mut counters = HashMap::new();
        counters.insert("section".into(), 4);
        counters.insert("paragraph".into(), 12);
        let c = Cursor {
            counters,
            tags: HashMap::new(),
        };
        let s = render_template(
            "§{{cursor.section}}.{{cursor.paragraph | pad2}}",
            &c,
            &HashMap::new(),
        );
        assert_eq!(s, "§4.12");
    }
}
