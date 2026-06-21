//! YAML frontmatter parser — inlined from `novel-knowledge::parser` to remove
//! the `novel-knowledge` dependency. Parses `---\n...\n---\nbody` into a typed
//! frontmatter struct + remaining body text.

use serde::de::DeserializeOwned;

/// Error during frontmatter parsing.
#[derive(Debug)]
pub enum FrontmatterError {
    /// Content does not start with `---`.
    MissingFrontmatter,
    /// Fewer than 3 `---`-delimited sections.
    MalformedFrontmatter,
    /// YAML block failed to deserialize into the expected type.
    ParseError {
        message: String,
        /// Approximate line number (1-based) of the YAML block.
        line: usize,
    },
}

impl std::fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFrontmatter => write!(f, "missing YAML frontmatter (---)"),
            Self::MalformedFrontmatter => write!(f, "malformed frontmatter delimiters"),
            Self::ParseError { message, line } => {
                write!(f, "YAML parse error at line ~{line}: {message}")
            }
        }
    }
}

/// Parse YAML frontmatter from a Markdown string.
///
/// Returns the deserialized frontmatter struct and the remaining body text
/// (everything after the closing `---`).
pub fn parse_frontmatter<T: DeserializeOwned>(
    content: &str,
) -> Result<(T, String), FrontmatterError> {
    if !content.starts_with("---") {
        return Err(FrontmatterError::MissingFrontmatter);
    }
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return Err(FrontmatterError::MalformedFrontmatter);
    }
    let yaml_str = parts[1].trim();
    let body = parts[2].trim().to_string();
    let frontmatter: T =
        serde_yaml::from_str(yaml_str).map_err(|e| FrontmatterError::ParseError {
            message: e.to_string(),
            line: estimate_line(content, yaml_str),
        })?;
    Ok((frontmatter, body))
}

/// Estimate the 1-based line number where `fragment` appears in `content`.
fn estimate_line(content: &str, fragment: &str) -> usize {
    let pos = content.find(fragment).unwrap_or(0);
    content[..pos].lines().count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestFm {
        name: String,
        count: u32,
    }

    #[test]
    fn parse_valid_frontmatter() {
        let content = "---\nname: test\ncount: 42\n---\n\nbody text here\nmore body";
        let (fm, body): (TestFm, _) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.name, "test");
        assert_eq!(fm.count, 42);
        assert_eq!(body, "body text here\nmore body");
    }

    #[test]
    fn missing_frontmatter_errors() {
        let result = parse_frontmatter::<TestFm>("no frontmatter here");
        assert!(matches!(result, Err(FrontmatterError::MissingFrontmatter)));
    }

    #[test]
    fn malformed_yaml_errors() {
        let content = "---\nname: [\n---\nbody";
        let result = parse_frontmatter::<TestFm>(content);
        assert!(matches!(result, Err(FrontmatterError::ParseError { .. })));
    }
}
