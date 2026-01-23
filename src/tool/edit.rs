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
                        "description": "The path to the file to modify (absolute or relative to working directory)"
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
        let file_path_arg = args
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

        // Resolve path using context helper
        let resolved_path = ctx.resolve_path(file_path_arg);

        // Validate path is within project root
        let path = validate_path(resolved_path.to_string_lossy().as_ref(), &ctx.root)?;
        let display_path = path.display().to_string();

        // Check if file exists
        if !path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", display_path));
        }

        // Read current content
        let content = fs::read_to_string(&path).await?;

        // Count occurrences
        let occurrences = content.matches(old_string).count();

        if occurrences == 0 {
            return Err(anyhow::anyhow!(
                "oldString not found in content.\n\nSearched for:\n{}\n\nMake sure the string matches exactly, including whitespace and indentation.",
                old_string
            ));
        }

        if occurrences > 1 && !replace_all {
            return Err(anyhow::anyhow!(
                "oldString found multiple times ({} occurrences) and requires more code context to uniquely identify the intended match.\n\nEither provide more surrounding lines in oldString to make it unique, or set replaceAll: true to replace all occurrences.",
                occurrences
            ));
        }

        // Request permission before editing
        let metadata = HashMap::from([
            ("filePath".to_string(), json!(display_path)),
            ("oldString".to_string(), json!(old_string)),
            ("newString".to_string(), json!(new_string)),
            ("replaceAll".to_string(), json!(replace_all)),
            ("occurrences".to_string(), json!(occurrences)),
        ]);

        if let Some(denied) = ctx
            .require_permission("edit", vec![display_path.clone()], metadata)
            .await?
        {
            return Ok(denied);
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
                format!("No changes to {}", display_path),
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
            format!("Edited {} ({} replacements)", display_path, occurrences)
        } else {
            format!("Edited {}", display_path)
        };

        let output = format!(
            "Successfully edited {}\nReplacements made: {}\nLines: {} -> {} ({:+})",
            display_path,
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
                m.insert("path".to_string(), json!(display_path));
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
