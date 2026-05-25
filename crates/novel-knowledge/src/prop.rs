use crate::{find_table_last_row, regex_cache, KnowledgeError, KnowledgeStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropRow {
    pub chapter: String,
    pub prop_name: String,
    pub holder: String,
    pub knowers: String,
}

pub fn parse_prop_table(content: &str) -> Vec<PropRow> {
    let re = regex_cache::four_col_table_re();
    let mut rows = Vec::new();
    for cap in re.captures_iter(content) {
        let c0 = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if c0 == "章节" || c0.starts_with("---") {
            continue;
        }
        rows.push(PropRow {
            chapter: c0.into(),
            prop_name: cap.get(2).map(|m| m.as_str().trim()).unwrap_or("").into(),
            holder: cap.get(3).map(|m| m.as_str().trim()).unwrap_or("").into(),
            knowers: cap.get(4).map(|m| m.as_str().trim()).unwrap_or("").into(),
        });
    }
    rows
}

pub fn read_props(store: &KnowledgeStore) -> Result<Vec<PropRow>, KnowledgeError> {
    let path = "knowledge/shared-systems/道具追踪.md";
    match store.read_file(path) {
        Ok(c) => Ok(parse_prop_table(&c)),
        Err(KnowledgeError::FileNotFound(_)) => Ok(vec![]),
        Err(e) => Err(e),
    }
}

pub fn last_prop_row(store: &KnowledgeStore) -> Result<Option<String>, KnowledgeError> {
    let content = store.read_file("knowledge/shared-systems/道具追踪.md")?;
    find_table_last_row(&content, "道具追踪日志")
}
