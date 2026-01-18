use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use yaml_rust2::YamlLoader;

/// Parsed markdown file with frontmatter
#[derive(Debug, Clone)]
pub struct MarkdownFile {
    /// Frontmatter metadata
    pub frontmatter: Frontmatter,
    /// Content after frontmatter
    pub content: String,
}

/// Frontmatter metadata from markdown files
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
}

/// Parse a markdown file with frontmatter
pub async fn parse_markdown_file(path: &Path) -> Result<MarkdownFile> {
    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file: {:?}", path))?;

    parse_markdown(&content)
}

/// Parse markdown content with frontmatter
pub fn parse_markdown(content: &str) -> Result<MarkdownFile> {
    // Check if content starts with frontmatter delimiter
    let content = content.trim_start();
    if !content.starts_with("---") {
        // No frontmatter, return entire content
        return Ok(MarkdownFile {
            frontmatter: Frontmatter::default(),
            content: content.to_string(),
        });
    }

    // Find the closing frontmatter delimiter (handle both LF and CRLF line endings)
    let after_opening = &content[3..]; // Skip opening "---"
    
    let (closing_pos, delimiter_len) = after_opening
        .find("\n---")
        .map(|pos| (pos, 4))
        .or_else(|| after_opening.find("\r\n---").map(|pos| (pos, 5)))
        .context("Frontmatter not properly closed with '---'")?;

    let frontmatter_str = &after_opening[..closing_pos];
    let remaining = &after_opening[closing_pos + delimiter_len..];

    // Parse YAML frontmatter
    let frontmatter = parse_frontmatter(frontmatter_str)?;

    Ok(MarkdownFile {
        frontmatter,
        content: remaining.trim().to_string(),
    })
}

/// Parse frontmatter YAML string
fn parse_frontmatter(yaml_str: &str) -> Result<Frontmatter> {
    if yaml_str.trim().is_empty() {
        return Ok(Frontmatter::default());
    }

    let docs = YamlLoader::load_from_str(yaml_str).context("Failed to parse YAML frontmatter")?;

    if docs.is_empty() {
        return Ok(Frontmatter::default());
    }

    let yaml = &docs[0];
    let mut frontmatter = Frontmatter::default();

    if let Some(hash) = yaml.as_hash() {
        for (key, value) in hash {
            if let Some(key_str) = key.as_str() {
                match key_str {
                    "description" => {
                        frontmatter.description = value.as_str().map(|s| s.to_string());
                    }
                    "agent" => {
                        frontmatter.agent = value.as_str().map(|s| s.to_string());
                    }
                    "model" => {
                        frontmatter.model = value.as_str().map(|s| s.to_string());
                    }
                    "subtask" => {
                        frontmatter.subtask = value.as_bool();
                    }
                    _ => {
                        // Ignore unknown fields
                    }
                }
            }
        }
    }

    Ok(frontmatter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_with_frontmatter() {
        let content = r#"---
description: Test command
model: anthropic/claude-3-5-sonnet-20241022
subtask: true
---

This is the command template.
$ARGUMENTS will be replaced.
"#;

        let parsed = parse_markdown(content).unwrap();
        assert_eq!(
            parsed.frontmatter.description,
            Some("Test command".to_string())
        );
        assert_eq!(
            parsed.frontmatter.model,
            Some("anthropic/claude-3-5-sonnet-20241022".to_string())
        );
        assert_eq!(parsed.frontmatter.subtask, Some(true));
        assert!(parsed.content.contains("This is the command template"));
    }

    #[test]
    fn test_parse_markdown_without_frontmatter() {
        let content = "Just plain content without frontmatter";
        let parsed = parse_markdown(content).unwrap();
        assert_eq!(parsed.frontmatter.description, None);
        assert_eq!(parsed.content, content);
    }

    #[test]
    fn test_parse_markdown_empty_frontmatter() {
        let content = r#"---
---

Content here
"#;

        let parsed = parse_markdown(content).unwrap();
        assert_eq!(parsed.frontmatter.description, None);
        assert!(parsed.content.contains("Content here"));
    }

    #[test]
    fn test_parse_frontmatter_with_agent() {
        let content = r#"---
description: git commit and push
agent: explorer
---

commit and push
"#;

        let parsed = parse_markdown(content).unwrap();
        assert_eq!(
            parsed.frontmatter.description,
            Some("git commit and push".to_string())
        );
        assert_eq!(parsed.frontmatter.agent, Some("explorer".to_string()));
    }
}
