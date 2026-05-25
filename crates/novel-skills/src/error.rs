#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Missing frontmatter in {0}")]
    MissingFrontmatter(String),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Skill not found: {0}")]
    NotFound(String),
}
