//! Read tool for reading file contents.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

/// Tool for reading files
pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self {
        Self
    }

    /// Find similar files in a directory for suggestions
    fn find_suggestions(dir: &Path, base: &str) -> Vec<String> {
        let base_lower = base.to_lowercase();
        let mut suggestions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let name_lower = name.to_lowercase();

                // Check if names are similar
                if name_lower.contains(&base_lower) || base_lower.contains(&name_lower) {
                    if let Some(path) = entry.path().to_str() {
                        suggestions.push(path.to_string());
                    }
                }
            }
        }

        suggestions.truncate(3);
        suggestions
    }
}

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_string(),
            description: r#"Reads a file from the local filesystem.
- The filePath parameter can be an absolute path or a relative path from the working directory
- By default, it reads up to 2000 lines starting from the beginning of the file
- You can optionally specify a line offset and limit (especially handy for long files)
- Results are returned with line numbers starting at 1
- You can read image files using this tool"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filePath": {
                        "type": "string",
                        "description": "The path to the file to read (absolute or relative to working directory)"
                    },
                    "offset": {
                        "type": "number",
                        "description": "The line number to start reading from (0-based)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "The number of lines to read (defaults to 2000)"
                    }
                },
                "required": ["filePath"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let file_path_arg = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("filePath is required"))?;

        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;

        // Resolve path: if not absolute, join with cwd (like TypeScript version)
        let file_path = Path::new(file_path_arg);
        let resolved_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            Path::new(&ctx.cwd).join(file_path)
        };

        // Validate path is within project root
        let path = validate_path(resolved_path.to_string_lossy().as_ref(), &ctx.root)?;

        // Check if file exists - provide suggestions if not found (like TypeScript version)
        if !path.exists() {
            let dir = path.parent().unwrap_or(Path::new(&ctx.root));
            let base = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let suggestions = Self::find_suggestions(dir, &base);

            let error_msg = if !suggestions.is_empty() {
                format!(
                    "File not found: {}\n\nDid you mean one of these?\n{}",
                    path.display(),
                    suggestions.join("\n")
                )
            } else {
                format!("File not found: {}", path.display())
            };

            return Err(anyhow::anyhow!(error_msg));
        }

        // Check if it's a directory
        if path.is_dir() {
            return Ok(ToolResult::error(
                format!("Is a directory: {}", path.display()),
                format!("'{}' is a directory, not a file", path.display()),
            ));
        }

        // Try to detect if it's a binary file
        let content = fs::read(&path).await?;

        // Check for binary content (null bytes in first 8KB)
        let is_binary = content.iter().take(8192).any(|&b| b == 0);

        if is_binary {
            // For binary files, return metadata
            let metadata = fs::metadata(&path).await?;
            let mime_type = mime_guess::from_path(&path)
                .first()
                .map(|m| m.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());

            return Ok(ToolResult::success(
                format!("Read binary file: {}", path.display()),
                format!(
                    "[Binary file: {} bytes, type: {}]",
                    metadata.len(),
                    mime_type
                ),
            )
            .with_metadata("binary", Value::Bool(true))
            .with_metadata("size", json!(metadata.len()))
            .with_metadata("mimeType", json!(mime_type)));
        }

        // Convert to string
        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();

        // Apply offset and limit
        let end = (offset + limit).min(total_lines);
        let selected_lines = if offset < total_lines {
            &lines[offset..end]
        } else {
            &[]
        };

        // Format with line numbers (cat -n style)
        let mut output = String::new();
        for (i, line) in selected_lines.iter().enumerate() {
            let line_num = offset + i + 1;
            // Truncate very long lines
            let line_content = if line.len() > 2000 {
                format!("{}...[truncated]", &line[..2000])
            } else {
                line.to_string()
            };
            output.push_str(&format!("{:6}\t{}\n", line_num, line_content));
        }

        let display_path = path.display().to_string();
        let title = if total_lines <= limit && offset == 0 {
            format!("Read {} ({} lines)", display_path, total_lines)
        } else {
            format!(
                "Read {} (lines {}-{} of {})",
                display_path,
                offset + 1,
                end,
                total_lines
            )
        };

        let (output, truncated) = truncate_output(&output);

        Ok(ToolResult {
            title,
            output,
            metadata: {
                let mut m = HashMap::new();
                m.insert("path".to_string(), json!(display_path));
                m.insert("totalLines".to_string(), json!(total_lines));
                m.insert("offset".to_string(), json!(offset));
                m.insert("limit".to_string(), json!(limit));
                m
            },
            truncated,
            attachments: Vec::new(),
        })
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}
