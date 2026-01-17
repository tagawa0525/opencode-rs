//! Tool execution logic for the agentic loop.
//!
//! This module handles executing tools called by the LLM and managing
//! the conversation state with tool results.

use super::*;
use crate::provider::{ChatContent, ChatMessage, ContentPart};
use anyhow::Result;
use std::collections::HashMap;

/// Represents a pending tool call that needs to be executed
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Execute a single tool call and return the result
pub async fn execute_tool(
    tool_name: &str,
    arguments: &str,
    _tool_id: &str,
    ctx: &ToolContext,
) -> Result<ToolResult> {
    // Parse arguments
    let args: serde_json::Value = serde_json::from_str(arguments)
        .unwrap_or_else(|_| serde_json::json!({}));

    // Get tool from registry and execute
    let registry = registry::registry();
    registry.execute(tool_name, args, ctx).await
}

/// Process all pending tool calls and add results to conversation
pub async fn execute_all_tools(
    pending_calls: Vec<PendingToolCall>,
    ctx: &ToolContext,
) -> Vec<ContentPart> {
    let mut results = Vec::new();

    for call in pending_calls {
        // Execute the tool
        let result = execute_tool(&call.name, &call.arguments, &call.id, ctx).await;

        // Convert to content part
        let content_part = match result {
            Ok(tool_result) => {
                // Format output as JSON with title and output
                let output_json = serde_json::json!({
                    "title": tool_result.title,
                    "output": tool_result.output,
                    "metadata": tool_result.metadata,
                    "truncated": tool_result.truncated,
                });

                ContentPart::ToolResult {
                    tool_use_id: call.id.clone(),
                    content: serde_json::to_string(&output_json).unwrap_or_else(|_| tool_result.output),
                    is_error: Some(false),
                }
            }
            Err(e) => {
                // Error result
                let error_json = serde_json::json!({
                    "title": "Tool Execution Error",
                    "error": e.to_string(),
                });

                ContentPart::ToolResult {
                    tool_use_id: call.id.clone(),
                    content: serde_json::to_string(&error_json).unwrap_or_else(|_| e.to_string()),
                    is_error: Some(true),
                }
            }
        };

        results.push(content_part);
    }

    results
}

/// Build a tool result message to send back to the LLM
pub fn build_tool_result_message(tool_results: Vec<ContentPart>) -> ChatMessage {
    ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Parts(tool_results),
    }
}

/// Extract pending tool calls from assistant message parts
pub fn extract_tool_calls_from_parts(parts: &[ContentPart]) -> Vec<PendingToolCall> {
    let mut calls = Vec::new();

    for part in parts {
        if let ContentPart::ToolUse { id, name, input } = part {
            calls.push(PendingToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
            });
        }
    }

    calls
}

/// Track tool calls during streaming
#[derive(Debug, Default)]
pub struct ToolCallTracker {
    /// Map of tool call ID to (name, arguments)
    calls: HashMap<String, (String, String)>,
}

impl ToolCallTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new tool call
    pub fn start_call(&mut self, id: String, name: String) {
        self.calls.insert(id, (name, String::new()));
    }

    /// Append arguments delta to a tool call
    pub fn add_arguments(&mut self, id: &str, delta: &str) {
        if let Some((_, args)) = self.calls.get_mut(id) {
            args.push_str(delta);
        }
    }

    /// Finalize a tool call and return it
    pub fn finish_call(&mut self, id: &str) -> Option<PendingToolCall> {
        self.calls.remove(id).map(|(name, arguments)| PendingToolCall {
            id: id.to_string(),
            name,
            arguments,
        })
    }

    /// Get all pending calls
    pub fn get_all_calls(&self) -> Vec<PendingToolCall> {
        self.calls
            .iter()
            .map(|(id, (name, arguments))| PendingToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            })
            .collect()
    }

    /// Clear all calls
    pub fn clear(&mut self) {
        self.calls.clear();
    }

    /// Check if there are any pending calls
    pub fn has_calls(&self) -> bool {
        !self.calls.is_empty()
    }
}
