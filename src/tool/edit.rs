//! Edit tool for making string replacements in files.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::fs;

/// Tool for editing files via string replacement
pub struct EditTool;

impl EditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for EditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit".to_string(),
            description: r#"Performs exact string replacements in files.
- The oldString must match exactly (including whitespace and indentation)
- The edit will FAIL if oldString is not found in the file
- The edit will FAIL if oldString appears multiple times (provide more context to make it unique)
- Use replaceAll: true to replace all occurrences
- ALWAYS prefer this tool over Write for modifying existing files"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filePath": {
                        "type": "string",
                        "description": "The absolute path to the file to modify"
                    },
                    "oldString": {
                        "type": "string",
                        "description": "The text to replace"
                    },
                    "newString": {
                        "type": "string",
                        "description": "The text to replace it with"
                    },
                    "replaceAll": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default: false)"
                    }
                },
                "required": ["filePath", "oldString", "newString"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let file_path = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("filePath is required"))?;

        let old_string = args
            .get("oldString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("oldString is required"))?;

        let new_string = args
            .get("newString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("newString is required"))?;

        let replace_all = args
            .get("replaceAll")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Validate path
        let path = validate_path(file_path, &ctx.root)?;

        // Check if file exists
        if !path.exists() {
            return Ok(ToolResult::error(
                format!("File not found: {}", file_path),
                format!("The file '{}' does not exist", file_path),
            ));
        }

        // Read current content
        let content = fs::read_to_string(&path).await?;

        // Count occurrences
        let occurrences = content.matches(old_string).count();

        if occurrences == 0 {
            return Ok(ToolResult::error(
                format!("String not found in {}", file_path),
                format!(
                    "The oldString was not found in the file content.\n\nSearched for:\n{}\n\nMake sure the string matches exactly, including whitespace and indentation.",
                    old_string
                ),
            ));
        }

        if occurrences > 1 && !replace_all {
            return Ok(ToolResult::error(
                format!("Multiple matches in {}", file_path),
                format!(
                    "The oldString was found {} times in the file.\n\nEither:\n1. Provide more context in oldString to make it unique, or\n2. Set replaceAll: true to replace all occurrences",
                    occurrences
                ),
            ));
        }

        // Perform replacement
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Check if anything changed
        if new_content == content {
            return Ok(ToolResult::success(
                format!("No changes to {}", file_path),
                "The oldString and newString are identical, no changes made.",
            ));
        }

        // Write the modified content
        fs::write(&path, &new_content).await?;

        // Calculate diff statistics
        let old_lines = content.lines().count();
        let new_lines = new_content.lines().count();
        let line_diff = new_lines as i64 - old_lines as i64;

        let title = if replace_all && occurrences > 1 {
            format!("Edited {} ({} replacements)", file_path, occurrences)
        } else {
            format!("Edited {}", file_path)
        };

        let output = format!(
            "Successfully edited {}\nReplacements made: {}\nLines: {} -> {} ({:+})",
            file_path,
            if replace_all { occurrences } else { 1 },
            old_lines,
            new_lines,
            line_diff
        );

        Ok(ToolResult {
            title,
            output,
            metadata: {
                let mut m = HashMap::new();
                m.insert("path".to_string(), json!(file_path));
                m.insert(
                    "replacements".to_string(),
                    json!(if replace_all { occurrences } else { 1 }),
                );
                m.insert("lineDiff".to_string(), json!(line_diff));
                m
            },
            truncated: false,
            attachments: Vec::new(),
        })
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}
