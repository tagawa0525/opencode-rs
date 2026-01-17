//! LLM streaming implementation for various providers.

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

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

/// Convert messages to OpenAI format
/// OpenAI expects:
/// - Tool calls as tool_calls array in assistant messages
/// - Tool results as separate messages with role="tool"
fn convert_messages_to_openai(messages: Vec<ChatMessage>) -> Vec<serde_json::Value> {
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

/// Streaming client for LLM APIs
pub struct StreamingClient {
    client: Client,
}

impl StreamingClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Stream from Anthropic API
    pub async fn stream_anthropic(
        &self,
        api_key: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": messages,
            "system": system,
            "tools": tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            }).collect::<Vec<_>>(),
            "stream": true,
        });

        let client = self.client.clone();
        let api_key = api_key.to_string();

        tokio::spawn(async move {
            let result = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,interleaved-thinking-2025-05-14",
                )
                .header("content-type", "application/json")
                .json(&request_body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let error = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string());
                        let _ = tx.send(StreamEvent::Error(error)).await;
                        return;
                    }

                    // Parse SSE stream
                    let mut bytes = response.bytes_stream();
                    use futures::StreamExt;

                    let mut buffer = String::new();

                    while let Some(chunk) = bytes.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                // Process complete SSE events
                                while let Some(pos) = buffer.find("\n\n") {
                                    let event = buffer[..pos].to_string();
                                    buffer = buffer[pos + 2..].to_string();

                                    if let Some(stream_event) = parse_anthropic_sse(&event) {
                                        if tx.send(stream_event).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    /// Stream from OpenAI-compatible API
    pub async fn stream_openai(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let openai_tools: Vec<_> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();

        // Convert messages to OpenAI format (handles tool results properly)
        let openai_messages = convert_messages_to_openai(messages);

        let request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": openai_messages,
            "tools": openai_tools,
            "stream": true,
            "stream_options": {
                "include_usage": true
            }
        });

        let client = self.client.clone();
        let api_key = api_key.to_string();
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        tokio::spawn(async move {
            let result = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&request_body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let error = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string());
                        let _ = tx.send(StreamEvent::Error(error)).await;
                        return;
                    }

                    let mut bytes = response.bytes_stream();
                    use futures::StreamExt;

                    let mut buffer = String::new();

                    while let Some(chunk) = bytes.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(pos) = buffer.find("\n") {
                                    let line = buffer[..pos].to_string();
                                    buffer = buffer[pos + 1..].to_string();

                                    if let Some(stream_event) = parse_openai_sse(&line) {
                                        if tx.send(stream_event).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    /// Stream from GitHub Copilot API (OpenAI-compatible)
    pub async fn stream_copilot(
        &self,
        token: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let openai_tools: Vec<_> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();

        // Convert messages to OpenAI format (handles tool results properly)
        let openai_messages = convert_messages_to_openai(messages);

        let request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": openai_messages,
            "tools": openai_tools,
            "stream": true,
        });

        let client = self.client.clone();
        let token = token.to_string();

        tokio::spawn(async move {
            let result = client
                .post("https://api.githubcopilot.com/chat/completions")
                .header("Authorization", format!("Bearer {}", token))
                .header("content-type", "application/json")
                .header("editor-version", "opencode/0.1.0")
                .header("copilot-integration-id", "vscode-chat")
                .json(&request_body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string());

                        // Enhanced error message for GitHub Copilot
                        let error = if error_text.contains("The requested model is not supported") {
                            format!("{}\n\nMake sure the model is enabled in your copilot settings: https://github.com/settings/copilot/features", error_text)
                        } else if status == 403 {
                            "Please reauthenticate with the copilot provider to ensure your credentials work properly with opencode-rs.".to_string()
                        } else {
                            error_text
                        };

                        let _ = tx.send(StreamEvent::Error(error)).await;
                        return;
                    }

                    let mut bytes = response.bytes_stream();
                    use futures::StreamExt;

                    let mut buffer = String::new();

                    while let Some(chunk) = bytes.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));

                                while let Some(pos) = buffer.find('\n') {
                                    let line = buffer[..pos].to_string();
                                    buffer = buffer[pos + 1..].to_string();

                                    if let Some(stream_event) = parse_openai_sse(&line) {
                                        if tx.send(stream_event).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }
}

impl Default for StreamingClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse Anthropic SSE event
fn parse_anthropic_sse(event: &str) -> Option<StreamEvent> {
    let mut event_type = None;
    let mut data = None;

    for line in event.lines() {
        if let Some(t) = line.strip_prefix("event: ") {
            event_type = Some(t.to_string());
        } else if let Some(d) = line.strip_prefix("data: ") {
            data = Some(d.to_string());
        }
    }

    let event_type = event_type?;
    let data = data?;

    match event_type.as_str() {
        "content_block_delta" => {
            let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
            let delta = parsed.get("delta")?;

            match delta.get("type")?.as_str()? {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    Some(StreamEvent::TextDelta(text))
                }
                "thinking_delta" => {
                    let text = delta.get("thinking")?.as_str()?.to_string();
                    Some(StreamEvent::ReasoningDelta(text))
                }
                "input_json_delta" => {
                    let partial = delta.get("partial_json")?.as_str()?.to_string();
                    let index = parsed.get("index")?.as_u64()? as usize;
                    Some(StreamEvent::ToolCallDelta {
                        id: format!("tool_{}", index),
                        arguments_delta: partial,
                    })
                }
                _ => None,
            }
        }
        "content_block_start" => {
            let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
            let content_block = parsed.get("content_block")?;

            if content_block.get("type")?.as_str()? == "tool_use" {
                let id = content_block.get("id")?.as_str()?.to_string();
                let name = content_block.get("name")?.as_str()?.to_string();
                Some(StreamEvent::ToolCallStart { id, name })
            } else {
                None
            }
        }
        "content_block_stop" => {
            let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
            let index = parsed.get("index")?.as_u64()? as usize;
            Some(StreamEvent::ToolCallEnd {
                id: format!("tool_{}", index),
            })
        }
        "message_delta" => {
            let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;

            if let Some(usage) = parsed.get("usage") {
                return Some(StreamEvent::Usage {
                    input_tokens: usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    cache_read_tokens: usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    cache_write_tokens: usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                });
            }

            let delta = parsed.get("delta")?;
            delta
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(|stop_reason| StreamEvent::Done {
                    finish_reason: stop_reason.to_string(),
                })
        }
        "message_stop" => Some(StreamEvent::Done {
            finish_reason: "stop".to_string(),
        }),
        "error" => {
            let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
            let message = parsed
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            Some(StreamEvent::Error(message))
        }
        _ => None,
    }
}

/// Parse OpenAI SSE event
fn parse_openai_sse(line: &str) -> Option<StreamEvent> {
    let data = line.strip_prefix("data: ")?;

    if data == "[DONE]" {
        return Some(StreamEvent::Done {
            finish_reason: "stop".to_string(),
        });
    }

    let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

    // Check for usage
    if let Some(usage) = parsed.get("usage") {
        return Some(StreamEvent::Usage {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output_tokens: usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_read_tokens: usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_write_tokens: 0,
        });
    }

    let choices = parsed.get("choices")?.as_array()?;
    let choice = choices.first()?;
    let delta = choice.get("delta")?;

    // Check for finish reason
    if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        if finish_reason != "null" {
            return Some(StreamEvent::Done {
                finish_reason: finish_reason.to_string(),
            });
        }
    }

    // Check for content
    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
        return Some(StreamEvent::TextDelta(content.to_string()));
    }

    // Check for tool calls
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for tool_call in tool_calls {
            let index = tool_call.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
            let id = tool_call
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("call_{}", index));

            if let Some(function) = tool_call.get("function") {
                if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                    return Some(StreamEvent::ToolCallStart {
                        id,
                        name: name.to_string(),
                    });
                }
                if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                    return Some(StreamEvent::ToolCallDelta {
                        id,
                        arguments_delta: arguments.to_string(),
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_anthropic_sse {
        use super::*;

        #[test]
        fn test_text_delta() {
            let event = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(result, Some(StreamEvent::TextDelta(text)) if text == "Hello"));
        }

        #[test]
        fn test_thinking_delta() {
            let event = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#;

            let result = parse_anthropic_sse(event);
            assert!(
                matches!(result, Some(StreamEvent::ReasoningDelta(text)) if text == "Let me think...")
            );
        }

        #[test]
        fn test_tool_use_start() {
            let event = r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_123","name":"bash"}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallStart { id, name })
                    if id == "tool_123" && name == "bash"
            ));
        }

        #[test]
        fn test_input_json_delta() {
            let event = r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\":"}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallDelta { id, arguments_delta })
                    if id == "tool_1" && arguments_delta == "{\"cmd\":"
            ));
        }

        #[test]
        fn test_message_stop() {
            let event = r#"event: message_stop
data: {}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_message_delta_with_stop_reason() {
            let event = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "end_turn"
            ));
        }

        #[test]
        fn test_usage() {
            let event = r#"event: message_delta
data: {"type":"message_delta","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Usage { input_tokens, output_tokens, cache_read_tokens, cache_write_tokens })
                    if input_tokens == 100 && output_tokens == 50 && cache_read_tokens == 10 && cache_write_tokens == 5
            ));
        }

        #[test]
        fn test_error_event() {
            let event = r#"event: error
data: {"error":{"message":"Rate limit exceeded"}}"#;

            let result = parse_anthropic_sse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Error(msg)) if msg == "Rate limit exceeded"
            ));
        }

        #[test]
        fn test_unknown_event() {
            let event = r#"event: ping
data: {}"#;

            let result = parse_anthropic_sse(event);
            assert!(result.is_none());
        }
    }

    mod parse_openai_sse {
        use super::*;

        #[test]
        fn test_done() {
            let line = "data: [DONE]";
            let result = parse_openai_sse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_text_delta() {
            let line = r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
            let result = parse_openai_sse(line);
            assert!(matches!(result, Some(StreamEvent::TextDelta(text)) if text == "Hello"));
        }

        #[test]
        fn test_finish_reason() {
            let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
            let result = parse_openai_sse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_tool_call_start() {
            let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_abc123","index":0,"function":{"name":"bash"}}]},"index":0}]}"#;
            let result = parse_openai_sse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallStart { id, name })
                    if id == "call_abc123" && name == "bash"
            ));
        }

        #[test]
        fn test_tool_call_arguments() {
            let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"cmd\":"}}]},"index":0}]}"#;
            let result = parse_openai_sse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallDelta { arguments_delta, .. })
                    if arguments_delta == "{\"cmd\":"
            ));
        }

        #[test]
        fn test_usage() {
            let line = r#"data: {"usage":{"prompt_tokens":100,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":10}}}"#;
            let result = parse_openai_sse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Usage { input_tokens, output_tokens, cache_read_tokens, .. })
                    if input_tokens == 100 && output_tokens == 50 && cache_read_tokens == 10
            ));
        }

        #[test]
        fn test_invalid_json() {
            let line = "data: not-json";
            let result = parse_openai_sse(line);
            assert!(result.is_none());
        }

        #[test]
        fn test_no_data_prefix() {
            let line = "not a data line";
            let result = parse_openai_sse(line);
            assert!(result.is_none());
        }
    }

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
