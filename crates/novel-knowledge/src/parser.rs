use crate::KnowledgeError;
use serde::de::DeserializeOwned;

pub fn parse_frontmatter<T: DeserializeOwned>(
    content: &str,
) -> Result<(T, String), KnowledgeError> {
    if !content.starts_with("---") {
        return Err(KnowledgeError::MissingFrontmatter);
    }
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return Err(KnowledgeError::MalformedFrontmatter);
    }
    let yaml_str = parts[1].trim();
    let body = parts[2].trim().to_string();
    let frontmatter: T =
        serde_yaml::from_str(yaml_str).map_err(|e| KnowledgeError::FrontmatterParseError {
            message: e.to_string(),
            line: estimate_line(content, yaml_str),
        })?;
    Ok((frontmatter, body))
}

fn estimate_line(content: &str, fragment: &str) -> usize {
    let pos = content.find(fragment).unwrap_or(0);
    content[..pos].lines().count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterFrontmatter;
    use rstest::rstest;

    #[rstest]
    #[test]
    fn parse_valid_frontmatter() {
        let content = r#"---
name: 林若烟
aliases: [若烟]
category: human
firstAppearance: chapter-003
lastUpdate: chapter-030
status: alive
povCharacter: true
---

## 身份
- 宗门: 青岚宗
"#;
        let (fm, body): (CharacterFrontmatter, _) = parse_frontmatter(content).unwrap();
        assert_eq!(fm.name, "林若烟");
        assert!(body.contains("青岚宗"));
    }

    #[rstest]
    #[test]
    fn missing_frontmatter_errors() {
        assert!(matches!(
            parse_frontmatter::<CharacterFrontmatter>("no frontmatter"),
            Err(KnowledgeError::MissingFrontmatter)
        ));
    }

    #[rstest]
    #[test]
    fn malformed_yaml_errors() {
        let content = "---\nname: [\n---\nbody";
        assert!(matches!(
            parse_frontmatter::<CharacterFrontmatter>(content),
            Err(KnowledgeError::FrontmatterParseError { .. })
        ));
    }
}
