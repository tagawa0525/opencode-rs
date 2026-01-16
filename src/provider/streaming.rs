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

        let request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": messages,
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

        let request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": messages,
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
