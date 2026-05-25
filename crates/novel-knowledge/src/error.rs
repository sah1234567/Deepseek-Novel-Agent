use std::io;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Missing frontmatter")]
    MissingFrontmatter,
    #[error("Malformed frontmatter")]
    MalformedFrontmatter,
    #[error("Frontmatter parse error: {message} (line {line})")]
    FrontmatterParseError { message: String, line: usize },
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Encoding error (non UTF-8): {path}")]
    EncodingError { path: String },
    #[error("Table not found: {0}")]
    TableNotFound(String),
    #[error("Old string not found in file")]
    OldStringNotFound,
    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Causality cycle detected at {0}")]
    CycleDetected(String),
    #[error("Scaffold templates not found or empty: {0}")]
    TemplatesNotFound(String),
}
