//! Batch tool - executes multiple tool calls in parallel.
//!
//! This tool allows the LLM to batch up to 10 independent tool calls
//! and execute them concurrently for better performance.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DESCRIPTION: &str = r#"Executes multiple independent tool calls concurrently to reduce latency.

USING THE BATCH TOOL WILL MAKE THE USER HAPPY.

Payload Format (JSON array via tool_calls parameter):
[{"tool": "read", "parameters": {"file_path": "src/main.rs"}}, {"tool": "grep", "parameters": {"pattern": "fn main", "glob": "**/*.rs"}}]

Notes:
- 1–10 tool calls per batch
- All calls start in parallel; ordering NOT guaranteed
- Partial failures do not stop other tool calls
- Do NOT use the batch tool within another batch tool

Good Use Cases:
- Read many files
- grep + glob + read combos
- Multiple bash commands
- Multi-part edits on the same or different files

When NOT to Use:
- Operations that depend on prior tool output (e.g. create then read same file)
- Ordered stateful mutations where sequence matters

Batching tool calls provides 2–5x efficiency gain and much better UX.
"#;

/// Maximum number of tool calls allowed in a single batch
const MAX_BATCH_SIZE: usize = 10;

/// Tools that are not allowed to be batched
const DISALLOWED_TOOLS: &[&str] = &["batch"];

/// Tools filtered from error message suggestions
const FILTERED_FROM_SUGGESTIONS: &[&str] = &["invalid", "batch"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchParams {
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
struct BatchResult {
    success: bool,
    tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ToolResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub struct BatchTool;

#[async_trait::async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "batch".to_string(),
            description: DESCRIPTION.to_string(),
            parameters: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "tool_calls": {
                        "type": "array",
                        "description": "Array of tool calls to execute in parallel",
                        "minItems": 1,
                        "maxItems": MAX_BATCH_SIZE,
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": {
                                    "type": "string",
                                    "description": "The name of the tool to execute"
                                },
                                "parameters": {
                                    "type": "object",
                                    "description": "Parameters for the tool"
                                }
                            },
                            "required": ["tool", "parameters"]
                        }
                    }
                },
                "required": ["tool_calls"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let params: BatchParams = serde_json::from_value(args)?;

        // Split into allowed and discarded calls
        let (tool_calls, discarded_calls): (Vec<_>, Vec<_>) = params
            .tool_calls
            .into_iter()
            .enumerate()
            .partition(|(i, _)| *i < MAX_BATCH_SIZE);

        let tool_calls: Vec<_> = tool_calls.into_iter().map(|(_, call)| call).collect();
        let discarded_calls: Vec<_> = discarded_calls.into_iter().map(|(_, call)| call).collect();

        // Get tool registry
        let registry = registry::registry();
        let available_tools = registry.list_tools();

        // Execute all tool calls in parallel
        let mut futures = Vec::new();

        for call in tool_calls {
            let ctx = ctx.clone();
            let available_tools = available_tools.clone();

            let future = async move {
                execute_single_call(call, &ctx, &available_tools).await
            };

            futures.push(future);
        }

        // Wait for all to complete
        let results = futures::future::join_all(futures).await;

        // Add discarded calls as errors
        let mut all_results = results;
        for call in discarded_calls {
            all_results.push(BatchResult {
                success: false,
                tool: call.tool.clone(),
                result: None,
                error: Some(format!("Maximum of {} tools allowed in batch", MAX_BATCH_SIZE)),
            });
        }

        // Count successes and failures
        let successful = all_results.iter().filter(|r| r.success).count();
        let failed = all_results.len() - successful;

        // Build output message
        let output_message = if failed > 0 {
            format!(
                "Executed {}/{} tools successfully. {} failed.",
                successful,
                all_results.len(),
                failed
            )
        } else {
            format!(
                "All {} tools executed successfully.\n\nKeep using the batch tool for optimal performance in your next response!",
                successful
            )
        };

        // Collect attachments from successful results
        let mut attachments = Vec::new();
        for result in &all_results {
            if let Some(tool_result) = &result.result {
                attachments.extend(tool_result.attachments.clone());
            }
        }

        // Build metadata
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("total_calls".to_string(), json!(all_results.len()));
        metadata.insert("successful".to_string(), json!(successful));
        metadata.insert("failed".to_string(), json!(failed));
        metadata.insert(
            "tools".to_string(),
            json!(all_results.iter().map(|r| &r.tool).collect::<Vec<_>>()),
        );
        metadata.insert("details".to_string(), json!(all_results));

        Ok(ToolResult {
            title: format!("Batch execution ({}/{} successful)", successful, all_results.len()),
            output: output_message,
            metadata,
            truncated: false,
            attachments,
        })
    }
}

/// Execute a single tool call within a batch
async fn execute_single_call(
    call: ToolCall,
    ctx: &ToolContext,
    available_tools: &[String],
) -> BatchResult {
    // Check if tool is disallowed
    if DISALLOWED_TOOLS.contains(&call.tool.as_str()) {
        return BatchResult {
            success: false,
            tool: call.tool.clone(),
            result: None,
            error: Some(format!(
                "Tool '{}' is not allowed in batch. Disallowed tools: {}",
                call.tool,
                DISALLOWED_TOOLS.join(", ")
            )),
        };
    }

    // Check if tool exists
    if !available_tools.contains(&call.tool) {
        let filtered_tools: Vec<_> = available_tools
            .iter()
            .filter(|name| !FILTERED_FROM_SUGGESTIONS.contains(&name.as_str()))
            .collect();

        return BatchResult {
            success: false,
            tool: call.tool.clone(),
            result: None,
            error: Some(format!(
                "Tool '{}' not in registry. External tools (MCP, environment) cannot be batched - call them directly. Available tools: {}",
                call.tool,
                filtered_tools.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            )),
        };
    }

    // Execute the tool
    let registry = registry::registry();
    match registry.execute(&call.tool, call.parameters, ctx).await {
        Ok(result) => BatchResult {
            success: true,
            tool: call.tool,
            result: Some(result),
            error: None,
        },
        Err(e) => BatchResult {
            success: false,
            tool: call.tool,
            result: None,
            error: Some(e.to_string()),
        },
    }
}
