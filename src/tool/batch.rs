//! Batch tool - executes multiple tool calls in parallel.
//!
//! This tool allows the LLM to batch up to 10 independent tool calls
//! and execute them concurrently for better performance.

use super::*;
use crate::id::{self, IdPrefix};
use crate::session::{
    Part, PartBase, ToolPart, ToolState, ToolStateCompleted, ToolStateError, ToolStateRunning,
    ToolTimeComplete, ToolTimeStart,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

const DESCRIPTION: &str = r#"Executes multiple independent tool calls concurrently to reduce latency.

USING THE BATCH TOOL WILL MAKE THE USER HAPPY.

Payload Format (JSON array via tool_calls parameter):
[{"tool": "read", "parameters": {"file_path": "src/main.rs"}}, {"tool": "grep", "parameters": {"pattern": "fn main", "glob": "**/*.rs"}}]

Notes:
- Supports unlimited tool calls (automatically calculates optimal batch size based on model context)
- Batch size is dynamically determined: targets 85% of model's context window
- Within each batch, all calls run in parallel; ordering NOT guaranteed
- Multiple batches are executed sequentially
- Partial failures do not stop other tool calls
- Do NOT use the batch tool within another batch tool

Batch Size Examples:
- Claude Sonnet 4: ~85 tool calls per batch (200K context)
- GPT-4: ~21 tool calls per batch (128K context)
- Smaller models: 5-10 tool calls per batch

Good Use Cases:
- Read many files (even 100+ files!)
- grep + glob + read combos
- Multiple bash commands
- Multi-part edits on the same or different files
- Checking multiple crates versions (webfetch calls)

When NOT to Use:
- Operations that depend on prior tool output (e.g. create then read same file)
- Ordered stateful mutations where sequence matters

Batching tool calls provides 2–5x efficiency gain and much better UX.
"#;

/// Default batch size when model context size is unknown
const DEFAULT_BATCH_SIZE: usize = 10;

/// Minimum batch size for safety
const MIN_BATCH_SIZE: usize = 5;

/// Maximum batch size to avoid API rate limits
const MAX_BATCH_SIZE: usize = 50;

/// Target context usage (85% to leave room for multibyte characters)
const TARGET_CONTEXT_USAGE: f64 = 0.85;

/// Estimated average tokens per tool result (conservative estimate)
const AVG_TOKENS_PER_TOOL_RESULT: u64 = 2000;

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
                        "description": "Array of tool calls to execute. Automatically splits into batches of 10 if more than 10 calls are provided.",
                        "minItems": 1,
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

        let total_calls = params.tool_calls.len();

        // Get tool registry
        let registry = registry::registry();
        let available_tools = registry.list_tools();

        // Calculate optimal batch size based on model context
        let batch_size = calculate_batch_size(ctx).await;

        tracing::debug!(
            "Calculated batch size: {} (total calls: {}, model: {:?})",
            batch_size,
            total_calls,
            ctx.model_id
        );

        // Split tool calls into batches
        let mut all_results = Vec::new();
        let batches: Vec<_> = params.tool_calls.chunks(batch_size).collect();

        let batch_count = batches.len();

        // Execute each batch sequentially (tools within each batch run in parallel)
        for (batch_idx, batch) in batches.into_iter().enumerate() {
            let mut futures = Vec::new();

            for call in batch {
                let mut ctx = ctx.clone();
                // Set is_in_batch flag for child tools (e.g., webfetch can use this to increase output limit)
                ctx.extra
                    .insert("is_in_batch".to_string(), serde_json::json!(true));
                let available_tools = available_tools.clone();
                let call = call.clone();

                let future = async move { execute_single_call(call, &ctx, &available_tools).await };

                futures.push(future);
            }

            // Wait for all calls in this batch to complete
            let batch_results = futures::future::join_all(futures).await;
            all_results.extend(batch_results);

            // If there are more batches, add a small note in the metadata
            if batch_idx < batch_count - 1 {
                // Could add logging here if needed
                tracing::debug!(
                    "Completed batch {}/{} ({} calls)",
                    batch_idx + 1,
                    batch_count,
                    batch.len()
                );
            }
        }

        // Count successes and failures
        let successful = all_results.iter().filter(|r| r.success).count();
        let failed = all_results.len() - successful;

        // Build output message
        let output_message = if batch_count > 1 {
            // Multiple batches executed
            if failed > 0 {
                format!(
                    "Executed {}/{} tools successfully across {} batches. {} failed.",
                    successful, total_calls, batch_count, failed
                )
            } else {
                format!(
                    "All {} tools executed successfully across {} batches.\n\nKeep using the batch tool for optimal performance in your next response!",
                    successful, batch_count
                )
            }
        } else {
            // Single batch
            if failed > 0 {
                format!(
                    "Executed {}/{} tools successfully. {} failed.",
                    successful, total_calls, failed
                )
            } else {
                format!(
                    "All {} tools executed successfully.\n\nKeep using the batch tool for optimal performance in your next response!",
                    successful
                )
            }
        };

        // Collect attachments from successful results
        let mut attachments = Vec::new();
        for result in &all_results {
            if let Some(tool_result) = &result.result {
                attachments.extend(tool_result.attachments.clone());
            }
        }

        // Build summarized details to avoid payload size issues
        let summarized_details: Vec<_> = all_results
            .iter()
            .map(|r| build_summarized_result(r))
            .collect();

        // Build metadata
        let mut metadata = HashMap::new();
        metadata.insert("total_calls".to_string(), json!(total_calls));
        metadata.insert("successful".to_string(), json!(successful));
        metadata.insert("failed".to_string(), json!(failed));
        metadata.insert("batch_count".to_string(), json!(batch_count));
        metadata.insert("batch_size".to_string(), json!(MAX_BATCH_SIZE));
        metadata.insert(
            "tools".to_string(),
            json!(all_results.iter().map(|r| &r.tool).collect::<Vec<_>>()),
        );
        metadata.insert("details".to_string(), json!(summarized_details));

        let title = if batch_count > 1 {
            format!(
                "Batch execution ({}/{} successful across {} batches)",
                successful, total_calls, batch_count
            )
        } else {
            format!(
                "Batch execution ({}/{} successful)",
                successful, total_calls
            )
        };

        let result = ToolResult {
            title,
            output: output_message,
            metadata,
            truncated: false,
            attachments,
        };

        // Log the size of the result for debugging
        if let Ok(json_str) = serde_json::to_string(&result) {
            let size_bytes = json_str.len();
            tracing::info!(
                "Batch tool result size: {} bytes ({} KB)",
                size_bytes,
                size_bytes / 1024
            );
        }

        Ok(result)
    }
}

/// Execute a single tool call within a batch
async fn execute_single_call(
    call: ToolCall,
    ctx: &ToolContext,
    available_tools: &[String],
) -> BatchResult {
    let call_start_time = chrono::Utc::now().timestamp_millis();
    let part_id = id::ascending(IdPrefix::Part);

    // Check if tool is disallowed
    if DISALLOWED_TOOLS.contains(&call.tool.as_str()) {
        let error_msg = format!(
            "Tool '{}' is not allowed in batch. Disallowed tools: {}",
            call.tool,
            DISALLOWED_TOOLS.join(", ")
        );

        let _ = save_tool_part(
            &part_id,
            ctx,
            &call.tool,
            call.parameters.clone(),
            call_start_time,
            ToolExecutionState::Error(&error_msg),
        )
        .await;

        return BatchResult {
            success: false,
            tool: call.tool.clone(),
            result: None,
            error: Some(error_msg),
        };
    }

    // Check if tool exists
    if !available_tools.contains(&call.tool) {
        let filtered_tools: Vec<_> = available_tools
            .iter()
            .filter(|name| !FILTERED_FROM_SUGGESTIONS.contains(&name.as_str()))
            .collect();

        let error_msg = format!(
            "Tool '{}' not in registry. External tools (MCP, environment) cannot be batched - call them directly. Available tools: {}",
            call.tool,
            filtered_tools.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );

        let _ = save_tool_part(
            &part_id,
            ctx,
            &call.tool,
            call.parameters.clone(),
            call_start_time,
            ToolExecutionState::Error(&error_msg),
        )
        .await;

        return BatchResult {
            success: false,
            tool: call.tool.clone(),
            result: None,
            error: Some(error_msg),
        };
    }

    // Note: We intentionally do NOT save individual tool results to avoid
    // adding them to the conversation context. Only the batch tool's summary
    // result should be added to the conversation to prevent payload overflow.

    // Execute the tool
    let registry = registry::registry();
    match registry
        .execute(&call.tool, call.parameters.clone(), ctx)
        .await
    {
        Ok(result) => BatchResult {
            success: true,
            tool: call.tool,
            result: Some(result),
            error: None,
        },
        Err(e) => {
            let error_msg = e.to_string();

            BatchResult {
                success: false,
                tool: call.tool,
                result: None,
                error: Some(error_msg),
            }
        }
    }
}

/// Tool execution state for saving to storage
enum ToolExecutionState<'a> {
    Running,
    Completed(&'a ToolResult),
    Error(&'a str),
}

/// Save tool part with the specified state
async fn save_tool_part(
    part_id: &str,
    ctx: &ToolContext,
    tool_name: &str,
    input: Value,
    start_time: i64,
    execution_state: ToolExecutionState<'_>,
) -> anyhow::Result<()> {
    let state = match execution_state {
        ToolExecutionState::Running => ToolState::Running(ToolStateRunning {
            input,
            title: None,
            metadata: None,
            time: ToolTimeStart { start: start_time },
        }),
        ToolExecutionState::Completed(result) => {
            let end_time = chrono::Utc::now().timestamp_millis();
            ToolState::Completed(ToolStateCompleted {
                input,
                output: result.output.clone(),
                title: result.title.clone(),
                metadata: result.metadata.clone(),
                time: ToolTimeComplete {
                    start: start_time,
                    end: end_time,
                    compacted: None,
                },
                attachments: None, // TODO: Convert result.attachments to FilePart
            })
        }
        ToolExecutionState::Error(error) => {
            let end_time = chrono::Utc::now().timestamp_millis();
            ToolState::Error(ToolStateError {
                input,
                error: error.to_string(),
                metadata: None,
                time: ToolTimeComplete {
                    start: start_time,
                    end: end_time,
                    compacted: None,
                },
            })
        }
    };

    let part = Part::Tool(ToolPart {
        base: PartBase {
            id: part_id.to_string(),
            session_id: ctx.session_id.clone(),
            message_id: ctx.message_id.clone(),
        },
        tool: tool_name.to_string(),
        call_id: part_id.to_string(),
        state,
        metadata: None,
    });

    part.save().await
}

/// Calculate optimal batch size based on model context size
async fn calculate_batch_size(ctx: &ToolContext) -> usize {
    // If no model ID is provided, use default
    let Some(model_id) = &ctx.model_id else {
        tracing::debug!("No model ID provided, using default batch size");
        return DEFAULT_BATCH_SIZE;
    };

    // Get model context size from models.dev
    match get_model_context_size(model_id).await {
        Ok(context_size) => {
            // Calculate usable context (85% of total)
            let usable_context = (context_size as f64 * TARGET_CONTEXT_USAGE) as u64;

            // Calculate how many tool results can fit
            let batch_size = (usable_context / AVG_TOKENS_PER_TOOL_RESULT) as usize;

            // Clamp to min/max bounds
            let clamped = batch_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);

            tracing::debug!(
                "Model {} context: {}, usable: {}, calculated batch size: {} (clamped: {})",
                model_id,
                context_size,
                usable_context,
                batch_size,
                clamped
            );

            clamped
        }
        Err(e) => {
            tracing::warn!(
                "Failed to get context size for model {}: {}. Using default batch size.",
                model_id,
                e
            );
            DEFAULT_BATCH_SIZE
        }
    }
}

/// Build a summarized result for metadata
fn build_summarized_result(r: &BatchResult) -> Value {
    if r.success {
        json!({
            "success": true,
            "tool": r.tool,
            "title": r.result.as_ref().map(|res| &res.title),
        })
    } else {
        json!({
            "success": false,
            "tool": r.tool,
            "error": r.error,
        })
    }
}
