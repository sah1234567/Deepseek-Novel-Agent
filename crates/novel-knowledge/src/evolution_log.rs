use crate::KnowledgeError;
use regex::Regex;
use std::sync::OnceLock;

fn table_row_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^\|[^\n]+\|$").expect("valid table row regex"))
}

pub trait TableRow {
    fn table_heading(&self) -> &str;
    fn to_markdown_row(&self) -> String;
}

/// Find the last data row of a markdown table under a heading.
pub fn find_table_last_row(
    content: &str,
    table_heading: &str,
) -> Result<Option<String>, KnowledgeError> {
    let heading = format!("## {}", table_heading);
    let Some(section_start) = content.find(&heading) else {
        return Err(KnowledgeError::TableNotFound(table_heading.into()));
    };
    let section = &content[section_start..];
    let table_re = table_row_re();
    let mut rows: Vec<&str> = table_re.find_iter(section).map(|m| m.as_str()).collect();
    if rows.len() <= 2 {
        // header + separator only
        return Ok(rows.first().map(|s| s.to_string()));
    }
    // skip header and separator
    Ok(rows.pop().map(|s| s.to_string()))
}

/// Append a row to an evolution log table (append-only semantics).
pub fn append_evolution_log(
    content: &str,
    table_heading: &str,
    new_row: &str,
) -> Result<String, KnowledgeError> {
    let last = find_table_last_row(content, table_heading)?;
    let old_string = last.ok_or(KnowledgeError::TableNotFound(table_heading.into()))?;
    if !content.contains(&old_string) {
        return Err(KnowledgeError::OldStringNotFound);
    }
    let new_string = format!("{old_string}\n{new_row}");
    Ok(content.replacen(&old_string, &new_string, 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    const SAMPLE: &str = r#"## 出场记录日志
| 章节 | 关键事件 | 伏笔关联 | 情绪弧线 |
|------|---------|---------|---------|
| Ch3  | 入门    | F03     | 好奇    |
| Ch7  | 发现    | F00     | 困惑    |
"#;

    #[rstest]
    #[test]
    fn find_last_row() {
        let last = find_table_last_row(SAMPLE, "出场记录日志")
            .unwrap()
            .unwrap();
        assert!(last.contains("Ch7"));
    }

    #[rstest]
    #[test]
    fn append_row() {
        let new_row = "| Ch31 | 对质 | F04 | 震惊 |";
        let updated = append_evolution_log(SAMPLE, "出场记录日志", new_row).unwrap();
        assert!(updated.contains(new_row));
        assert!(updated.contains("Ch7"));
    }

    #[rstest]
    #[test]
    fn empty_table_only_header() {
        let content = "## 出场记录日志\n| 章节 | 事件 |\n|------|------|\n";
        let last = find_table_last_row(content, "出场记录日志").unwrap();
        assert!(last.is_some());
    }

    #[rstest]
    #[test]
    fn missing_table_errors() {
        assert!(matches!(
            find_table_last_row(SAMPLE, "不存在"),
            Err(KnowledgeError::TableNotFound(_))
        ));
    }
}
