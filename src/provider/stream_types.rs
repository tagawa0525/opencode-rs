//! Stream types for LLM API communication.
//!
//! This module contains type definitions for streaming responses from LLM providers,
//! including message formats and tool definitions.

use serde::{Deserialize, Serialize};

/// Stream event from LLM
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text content delta
    TextDelta(String),
    /// Reasoning/thinking content delta
    ReasoningDelta(String),
    /// Tool call started
    ToolCallStart { id: String, name: String },
    /// Tool call argument delta
    ToolCallDelta { id: String, arguments_delta: String },
    /// Tool call completed
    ToolCallEnd { id: String },
    /// Tool result ready (for manual execution)
    ToolResult {
        id: String,
        result: String,
        is_error: bool,
    },
    /// Usage information
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    },
    /// Stream finished
    Done { finish_reason: String },
    /// Error occurred
    Error(String),
}

/// Message format for API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Tool definition for API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Convert messages to OpenAI format with optional system prompt
/// OpenAI expects:
/// - System message as the first message with role="system"
/// - Tool calls as tool_calls array in assistant messages
/// - Tool results as separate messages with role="tool"
pub fn convert_messages_to_openai_with_system(
    messages: Vec<ChatMessage>,
    system: Option<String>,
) -> Vec<serde_json::Value> {
    let mut result = Vec::new();

    // Add system message first if provided
    if let Some(system_prompt) = system {
        result.push(serde_json::json!({
            "role": "system",
            "content": system_prompt,
        }));
    }

    result.extend(convert_messages_to_openai(messages));
    result
}

/// Convert messages to OpenAI format
/// OpenAI expects:
/// - Tool calls as tool_calls array in assistant messages
/// - Tool results as separate messages with role="tool"
pub fn convert_messages_to_openai(messages: Vec<ChatMessage>) -> Vec<serde_json::Value> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.content {
            ChatContent::Text(text) => {
                result.push(serde_json::json!({
                    "role": msg.role,
                    "content": text,
                }));
            }
            ChatContent::Parts(parts) => {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();

                for part in parts {
                    match part {
                        ContentPart::Text { text } => {
                            text_parts.push(serde_json::json!({
                                "type": "text",
                                "text": text,
                            }));
                        }
                        ContentPart::ImageUrl { image_url } => {
                            text_parts.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": image_url,
                            }));
                        }
                        ContentPart::ToolUse { id, name, input } => {
                            // OpenAI uses tool_calls array, not content parts
                            tool_calls.push(serde_json::json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_default(),
                                }
                            }));
                        }
                        ContentPart::ToolResult {
                            tool_use_id,
                            content,
                            is_error: _,
                        } => {
                            // OpenAI expects tool results as separate messages
                            tool_results.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content,
                            }));
                        }
                    }
                }

                // Add main message if it has content or tool calls
                if !text_parts.is_empty() || !tool_calls.is_empty() {
                    let mut message = serde_json::json!({
                        "role": msg.role,
                    });

                    // Add content if we have text/image parts
                    if !text_parts.is_empty() {
                        if text_parts.len() == 1 && text_parts[0]["type"] == "text" {
                            // Simple text content
                            message["content"] = text_parts[0]["text"].clone();
                        } else {
                            // Multiple parts or images
                            message["content"] = serde_json::json!(text_parts);
                        }
                    } else if tool_calls.is_empty() {
                        // No content and no tool calls - add empty content
                        message["content"] = serde_json::json!("");
                    }

                    // Add tool_calls if we have any
                    if !tool_calls.is_empty() {
                        message["tool_calls"] = serde_json::json!(tool_calls);
                    }

                    result.push(message);
                }

                // Add tool result messages
                result.extend(tool_results);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    mod convert_messages_to_openai {
        use super::*;

        #[test]
        fn test_simple_text_message() {
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text("Hello".to_string()),
            }];

            let result = convert_messages_to_openai(messages);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0]["role"], "user");
            assert_eq!(result[0]["content"], "Hello");
        }

        #[test]
        fn test_message_with_tool_calls() {
            let messages = vec![ChatMessage {
                role: "assistant".to_string(),
                content: ChatContent::Parts(vec![
                    ContentPart::Text {
                        text: "Let me help.".to_string(),
                    },
                    ContentPart::ToolUse {
                        id: "call_123".to_string(),
                        name: "bash".to_string(),
                        input: serde_json::json!({"cmd": "ls"}),
                    },
                ]),
            }];

            let result = convert_messages_to_openai(messages);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0]["role"], "assistant");
            assert_eq!(result[0]["content"], "Let me help.");
            assert!(result[0]["tool_calls"].is_array());
        }

        #[test]
        fn test_message_with_tool_results() {
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Parts(vec![ContentPart::ToolResult {
                    tool_use_id: "call_123".to_string(),
                    content: "file.txt".to_string(),
                    is_error: None,
                }]),
            }];

            let result = convert_messages_to_openai(messages);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0]["role"], "tool");
            assert_eq!(result[0]["tool_call_id"], "call_123");
            assert_eq!(result[0]["content"], "file.txt");
        }

        #[test]
        fn test_message_with_image() {
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Parts(vec![
                    ContentPart::Text {
                        text: "What is this?".to_string(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl {
                            url: "data:image/png;base64,abc".to_string(),
                            detail: Some("auto".to_string()),
                        },
                    },
                ]),
            }];

            let result = convert_messages_to_openai(messages);
            assert_eq!(result.len(), 1);
            assert!(result[0]["content"].is_array());
        }
    }
}
