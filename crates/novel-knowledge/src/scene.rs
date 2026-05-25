use crate::{find_table_last_row, regex_cache, KnowledgeError, KnowledgeStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRow {
    pub chapter: String,
    pub location: String,
    pub characters: String,
    pub knowers: String,
}

const TABLE: &str = "场景追踪日志";

pub fn parse_scene_table(content: &str) -> Vec<SceneRow> {
    let re = regex_cache::four_col_table_re();
    let mut rows = Vec::new();
    for cap in re.captures_iter(content) {
        let c0 = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if c0 == "章节" || c0.starts_with("---") {
            continue;
        }
        rows.push(SceneRow {
            chapter: c0.into(),
            location: cap.get(2).map(|m| m.as_str().trim()).unwrap_or("").into(),
            characters: cap.get(3).map(|m| m.as_str().trim()).unwrap_or("").into(),
            knowers: cap.get(4).map(|m| m.as_str().trim()).unwrap_or("").into(),
        });
    }
    rows
}

pub fn read_scenes(store: &KnowledgeStore) -> Result<Vec<SceneRow>, KnowledgeError> {
    let path = "knowledge/shared-systems/场景追踪.md";
    match store.read_file(path) {
        Ok(c) => Ok(parse_scene_table(&c)),
        Err(KnowledgeError::FileNotFound(_)) => Ok(vec![]),
        Err(e) => Err(e),
    }
}

pub fn last_scene_row(store: &KnowledgeStore) -> Result<Option<String>, KnowledgeError> {
    let content = store.read_file("knowledge/shared-systems/场景追踪.md")?;
    find_table_last_row(&content, TABLE)
}
