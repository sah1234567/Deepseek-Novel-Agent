use crate::{regex_cache, KnowledgeError, KnowledgeStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionRow {
    pub faction: String,
    pub chapter: String,
    pub event: String,
    pub relation: String,
}

pub fn parse_faction_table(content: &str) -> Vec<FactionRow> {
    let re = regex_cache::four_col_table_re();
    let mut rows = Vec::new();
    for cap in re.captures_iter(content) {
        let c0 = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if c0 == "势力" || c0.starts_with("---") {
            continue;
        }
        rows.push(FactionRow {
            faction: c0.into(),
            chapter: cap.get(2).map(|m| m.as_str().trim()).unwrap_or("").into(),
            event: cap.get(3).map(|m| m.as_str().trim()).unwrap_or("").into(),
            relation: cap.get(4).map(|m| m.as_str().trim()).unwrap_or("").into(),
        });
    }
    rows
}

pub fn read_factions(store: &KnowledgeStore) -> Result<Vec<FactionRow>, KnowledgeError> {
    let path = "knowledge/shared-systems/势力追踪.md";
    match store.read_file(path) {
        Ok(c) => Ok(parse_faction_table(&c)),
        Err(KnowledgeError::FileNotFound(_)) => Ok(vec![]),
        Err(e) => Err(e),
    }
}
