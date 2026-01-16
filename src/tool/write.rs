//! Write tool for creating/overwriting files.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

/// Tool for writing files
pub struct WriteTool;

impl WriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn id(&self) -> &str {
        "write"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write".to_string(),
            description: r#"Writes content to a file on the local filesystem.
- This tool will overwrite the existing file if there is one at the provided path
- ALWAYS prefer editing existing files in the codebase using the Edit tool
- NEVER proactively create documentation files unless explicitly requested
- The filePath must be an absolute path"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filePath": {
                        "type": "string",
                        "description": "The absolute path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["filePath", "content"]
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let file_path = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("filePath is required"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("content is required"))?;

        // Validate path (allow paths outside root for absolute paths)
        let path = Path::new(file_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Check if file already exists
        let existed = path.exists();
        let old_size: u64 = if existed {
            tokio::fs::metadata(&path).await?.len()
        } else {
            0
        };

        // Write the file
        fs::write(&path, content).await?;

        let lines = content.lines().count();
        let bytes = content.len();

        let title = if existed {
            format!("Updated {} ({} lines)", file_path, lines)
        } else {
            format!("Created {} ({} lines)", file_path, lines)
        };

        Ok(ToolResult {
            title,
            output: format!(
                "Successfully {} file: {}\nBytes written: {}{}",
                if existed { "updated" } else { "created" },
                file_path,
                bytes,
                if existed {
                    format!(" (was {} bytes)", old_size)
                } else {
                    String::new()
                }
            ),
            metadata: {
                let mut m = HashMap::new();
                m.insert("path".to_string(), json!(file_path));
                m.insert("created".to_string(), json!(!existed));
                m.insert("lines".to_string(), json!(lines));
                m.insert("bytes".to_string(), json!(bytes));
                m
            },
            truncated: false,
            attachments: Vec::new(),
        })
    }
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}
