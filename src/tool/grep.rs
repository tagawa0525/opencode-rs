//! Grep tool for searching file contents.

use super::*;
use anyhow::Result;
use ::glob::Pattern;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};

/// Tool for searching file contents with regex
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: r#"Fast content search tool that works with any codebase size.
- Searches file contents using regular expressions
- Supports full regex syntax (e.g., "log.*Error", "function\s+\w+")
- Filter files by pattern with the include parameter (e.g., "*.rs", "*.{ts,tsx}")
- Returns file paths and line numbers with matches
- Respects .gitignore by default"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The regex pattern to search for in file contents"
                    },
                    "path": {
                        "type": "string",
                        "description": "The directory to search in (defaults to current directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "File pattern to include (e.g., \"*.rs\", \"*.{ts,tsx}\")"
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

        let search_path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ctx.cwd.clone());

        let include_pattern = args.get("include").and_then(|v| v.as_str());

        // Compile regex
        let regex = Regex::new(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid regex pattern '{}': {}", pattern, e))?;

        // Build glob matcher for include pattern
        let include_glob: Option<Pattern> = include_pattern
            .and_then(|p| Pattern::new(p).ok());

        // Walk directory respecting .gitignore
        let walker = WalkBuilder::new(&search_path)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .build();

        let mut results: Vec<SearchResult> = Vec::new();
        let max_results = 500;
        let max_matches_per_file = 10;

        'outer: for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            // Check include pattern
            if let Some(ref glob) = include_glob {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !glob.matches(filename) {
                    continue;
                }
            }

            // Skip binary files (simple heuristic)
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            let binary_extensions = [
                "png", "jpg", "jpeg", "gif", "ico", "woff", "woff2", "ttf", "eot", "pdf", "zip",
                "tar", "gz", "exe", "dll", "so", "dylib", "bin", "dat",
            ];
            if binary_extensions.contains(&ext.to_lowercase().as_str()) {
                continue;
            }

            // Read file content
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue, // Skip files that can't be read as text
            };

            // Search for matches
            let relative_path = path
                .strip_prefix(&search_path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());

            let mut file_matches = 0;
            for (line_num, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(SearchResult {
                        path: relative_path.clone(),
                        line_number: line_num + 1,
                        line_content: line.chars().take(200).collect(), // Truncate long lines
                    });

                    file_matches += 1;
                    if file_matches >= max_matches_per_file {
                        break;
                    }

                    if results.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }

        // Sort by path, then line number
        results.sort_by(|a, b| a.path.cmp(&b.path).then(a.line_number.cmp(&b.line_number)));

        let total_count = results.len();
        let truncated = total_count >= max_results;

        // Format output
        let output = if results.is_empty() {
            format!(
                "No matches found for pattern '{}' in {}",
                pattern, search_path
            )
        } else {
            results
                .iter()
                .map(|r| format!("{}:{}: {}", r.path, r.line_number, r.line_content))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let title = if total_count == 0 {
            format!("No matches for '{}'", pattern)
        } else if truncated {
            format!("Found {}+ matches for '{}'", total_count, pattern)
        } else {
            format!("Found {} matches for '{}'", total_count, pattern)
        };

        Ok(ToolResult {
            title,
            output,
            metadata: {
                let mut m = HashMap::new();
                m.insert("pattern".to_string(), json!(pattern));
                m.insert("path".to_string(), json!(search_path));
                m.insert("count".to_string(), json!(total_count));
                m.insert("truncated".to_string(), json!(truncated));
                if let Some(include) = include_pattern {
                    m.insert("include".to_string(), json!(include));
                }
                m
            },
            truncated,
            attachments: Vec::new(),
        })
    }
}

#[derive(Debug)]
struct SearchResult {
    path: String,
    line_number: usize,
    line_content: String,
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}
