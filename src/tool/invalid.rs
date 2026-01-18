//! Invalid tool - handles malformed tool calls.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};

/// Tool for handling invalid tool calls
pub struct InvalidTool;

impl InvalidTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for InvalidTool {
    fn id(&self) -> &str {
        "invalid"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "invalid".to_string(),
            description: "Do not use - internal tool for handling invalid calls".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool": {
                        "type": "string",
                        "description": "The tool that was called"
                    },
                    "error": {
                        "type": "string",
                        "description": "The error message"
                    }
                },
                "required": ["tool", "error"]
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let tool = args
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let error = args
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        Ok(ToolResult::error(
            "Invalid Tool",
            format!("The arguments provided to the tool are invalid: {}", error),
        )
        .with_metadata("tool", json!(tool)))
    }
}

impl Default for InvalidTool {
    fn default() -> Self {
        Self::new()
    }
}
