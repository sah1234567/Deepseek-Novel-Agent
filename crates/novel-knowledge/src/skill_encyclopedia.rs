use crate::{regex_cache, KnowledgeError, KnowledgeStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub rank: String,
    pub description: String,
}

pub fn parse_skill_encyclopedia(content: &str) -> Vec<SkillEntry> {
    let re = regex_cache::three_col_table_re();
    let mut entries = Vec::new();
    for cap in re.captures_iter(content) {
        let name = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if name == "功法名" || name.starts_with("---") {
            continue;
        }
        entries.push(SkillEntry {
            name: name.into(),
            rank: cap.get(2).map(|m| m.as_str().trim()).unwrap_or("").into(),
            description: cap.get(3).map(|m| m.as_str().trim()).unwrap_or("").into(),
        });
    }
    entries
}

pub fn read_skills(store: &KnowledgeStore) -> Result<Vec<SkillEntry>, KnowledgeError> {
    let path = "knowledge/shared-systems/功法技能.md";
    match store.read_file(path) {
        Ok(c) => Ok(parse_skill_encyclopedia(&c)),
        Err(KnowledgeError::FileNotFound(_)) => Ok(vec![]),
        Err(e) => Err(e),
    }
}

pub fn skill_exists(store: &KnowledgeStore, name: &str) -> Result<bool, KnowledgeError> {
    let skills = read_skills(store)?;
    Ok(skills.iter().any(|s| s.name == name))
}
