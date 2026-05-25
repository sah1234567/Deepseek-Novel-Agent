use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CharacterFrontmatter {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub category: CharacterCategory,
    #[serde(rename = "firstAppearance")]
    pub first_appearance: String,
    #[serde(rename = "lastUpdate")]
    pub last_update: String,
    pub status: CharacterStatus,
    #[serde(rename = "povCharacter")]
    pub pov_character: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CharacterCategory {
    Human,
    Spirit,
    System,
    Beast,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CharacterStatus {
    Alive,
    Dead,
    Missing,
    Unknown,
}
