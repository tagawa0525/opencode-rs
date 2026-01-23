//! Glob tool for finding files by pattern.

use super::*;
use ::glob::glob as glob_match;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

/// Tool for finding files by glob pattern
pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "glob".to_string(),
            description: r#"Fast file pattern matching tool that works with any codebase size.
- Supports glob patterns like "**/*.rs" or "src/**/*.ts"
- Returns matching file paths sorted by modification time
- Use this tool when you need to find files by name patterns
- Respects .gitignore by default"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The glob pattern to match files against"
                    },
                    "path": {
                        "type": "string",
                        "description": "The directory to search in (defaults to current directory)"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("pattern is required"))?;

        // Resolve search path using context helper
        let search_path_arg = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.cwd);

        let search_path = ctx
            .resolve_path(search_path_arg)
            .to_string_lossy()
            .to_string();

        // Request permission before globbing
        let metadata = HashMap::from([
            ("pattern".to_string(), json!(pattern)),
            ("path".to_string(), json!(search_path)),
        ]);

        if let Some(denied) = ctx
            .require_permission("glob", vec![pattern.to_string()], metadata)
            .await?
        {
            return Ok(denied);
        }

        // Build the glob pattern
        let glob_pattern = if Path::new(pattern).is_absolute() {
            pattern.to_string()
        } else {
            format!("{}/{}", search_path, pattern)
        };

        // Use the glob crate for pattern matching
        let matcher = glob_match(&glob_pattern)?;

        let mut files: Vec<(String, std::time::SystemTime)> = Vec::new();

        for entry in matcher {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        let mtime = path
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

                        // Make path relative to search_path if possible
                        let display_path = path
                            .strip_prefix(&search_path)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| path.to_string_lossy().to_string());

                        files.push((display_path, mtime));
                    }
                }
                Err(_) => continue,
            }
        }

        // Sort by modification time (newest first)
        files.sort_by(|a, b| b.1.cmp(&a.1));

        let total_count = files.len();

        // Limit output
        let max_files = 1000;
        let truncated = files.len() > max_files;
        if truncated {
            files.truncate(max_files);
        }

        let output = files
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let title = if total_count == 0 {
            format!("No files matching '{}'", pattern)
        } else if truncated {
            format!(
                "Found {} files matching '{}' (showing first {})",
                total_count, pattern, max_files
            )
        } else {
            format!("Found {} files matching '{}'", total_count, pattern)
        };

        Ok(ToolResult {
            title,
            output: if total_count == 0 {
                format!(
                    "No files found matching pattern '{}' in {}",
                    pattern, search_path
                )
            } else {
                output
            },
            metadata: {
                let mut m = HashMap::new();
                m.insert("pattern".to_string(), json!(pattern));
                m.insert("path".to_string(), json!(search_path));
                m.insert("count".to_string(), json!(total_count));
                m.insert("truncated".to_string(), json!(truncated));
                m
            },
            truncated,
            attachments: Vec::new(),
        })
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}
