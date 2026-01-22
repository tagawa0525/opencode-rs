//! Write tool for creating/overwriting files.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
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
- The filePath can be absolute or relative to the working directory"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filePath": {
                        "type": "string",
                        "description": "The path to the file to write (absolute or relative to working directory)"
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

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let file_path_arg = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("filePath is required"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("content is required"))?;

        // Resolve path using context helper
        let resolved_path = ctx.resolve_path(file_path_arg);

        // Validate path is within project root
        let path = validate_path(resolved_path.to_string_lossy().as_ref(), &ctx.root)?;

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

        let display_path = path.display().to_string();

        // Request permission before writing
        let metadata = HashMap::from([
            ("filePath".to_string(), json!(display_path)),
            ("existed".to_string(), json!(existed)),
            ("contentLength".to_string(), json!(content.len())),
        ]);

        if let Some(denied) = ctx
            .require_permission("write", vec![display_path.clone()], metadata)
            .await?
        {
            return Ok(denied);
        }

        // Write the file
        fs::write(&path, content).await?;

        let lines = content.lines().count();
        let bytes = content.len();

        let title = if existed {
            format!("Updated {} ({} lines)", display_path, lines)
        } else {
            format!("Created {} ({} lines)", display_path, lines)
        };

        Ok(ToolResult {
            title,
            output: format!(
                "Successfully {} file: {}\nBytes written: {}{}",
                if existed { "updated" } else { "created" },
                display_path,
                bytes,
                if existed {
                    format!(" (was {} bytes)", old_size)
                } else {
                    String::new()
                }
            ),
            metadata: {
                let mut m = HashMap::new();
                m.insert("path".to_string(), json!(display_path));
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
