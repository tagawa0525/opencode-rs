//! LLM streaming implementation for various providers.
//!
//! This module provides the `StreamingClient` for streaming responses from
//! different LLM providers (Anthropic, OpenAI, GitHub Copilot).

use anyhow::Result;
use futures::StreamExt;
use reqwest::{Client, Response};
use tokio::sync::mpsc;

// Re-export types from stream_types for convenience
pub use super::parsers::{AnthropicParser, OpenAIParser};
pub use super::stream_types::*;

/// Parameters for OpenAI-compatible API streaming
pub struct OpenAIStreamParams {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u64,
    pub request_modifier:
        Option<Box<dyn FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send>>,
}

/// Parser trait for SSE streams
trait SseParser: Send + 'static {
    fn parse(&mut self, chunk: &str) -> Option<StreamEvent>;
    fn event_delimiter(&self) -> &str;
}

impl SseParser for AnthropicParser {
    fn parse(&mut self, chunk: &str) -> Option<StreamEvent> {
        self.parse(chunk)
    }

    fn event_delimiter(&self) -> &str {
        "\n\n" // Anthropic uses double newline for event separation
    }
}

impl SseParser for OpenAIParser {
    fn parse(&mut self, chunk: &str) -> Option<StreamEvent> {
        self.parse(chunk)
    }

    fn event_delimiter(&self) -> &str {
        "\n" // OpenAI uses single newline
    }
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

    /// Generic SSE stream processor
    async fn process_sse_stream<P: SseParser>(
        response: Response,
        tx: mpsc::Sender<StreamEvent>,
        mut parser: P,
    ) {
        let mut bytes = response.bytes_stream();
        let mut buffer = String::new();
        let delimiter = parser.event_delimiter().to_string();
        let delimiter_len = delimiter.len();

        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(pos) = buffer.find(&delimiter) {
                        let event = buffer[..pos].to_string();
                        buffer = buffer[pos + delimiter_len..].to_string();

                        if let Some(stream_event) = parser.parse(&event) {
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

        // Check request body size (5MB limit for Anthropic API)
        let request_body_str = serde_json::to_string(&request_body)?;
        let request_size = request_body_str.len();
        const MAX_REQUEST_SIZE: usize = 5 * 1024 * 1024; // 5MB

        if request_size > MAX_REQUEST_SIZE {
            // Count tool calls to provide better guidance
            let tool_call_count = request_body
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|msgs| msgs.last())
                .and_then(|msg| msg.get("content"))
                .and_then(|content| content.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .count()
                })
                .unwrap_or(0);

            let error_msg = if tool_call_count > 10 {
                format!(
                    "Request payload too large: {} bytes (max: {} bytes). \
                    You are trying to make {} tool calls at once. \
                    \n\nIMPORTANT: Use the 'batch' tool instead! \
                    \n\nExample: \
                    \n{{\n  \"tool\": \"batch\",\n  \"parameters\": {{\n    \"tool_calls\": [\n      {{\"tool\": \"webfetch\", \"parameters\": {{...}}}},\n      {{\"tool\": \"webfetch\", \"parameters\": {{...}}}}\n    ]\n  }}\n}} \
                    \n\nThe batch tool automatically handles large numbers of tool calls efficiently.",
                    request_size, MAX_REQUEST_SIZE, tool_call_count
                )
            } else {
                format!(
                    "Request payload too large: {} bytes (max: {} bytes). \
                    This usually happens when trying to make too many tool calls at once. \
                    Please use the 'batch' tool to execute multiple tools efficiently, or break down your request into smaller steps.",
                    request_size, MAX_REQUEST_SIZE
                )
            };
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                let _ = tx_clone.send(StreamEvent::Error(error_msg)).await;
            });
            return Ok(rx);
        }

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

                    Self::process_sse_stream(response, tx, AnthropicParser::new()).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    /// Stream from OpenAI-compatible API
    #[allow(clippy::too_many_arguments)]
    pub async fn stream_openai(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let params = OpenAIStreamParams {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            messages,
            system,
            tools,
            max_tokens,
            request_modifier: None,
        };
        self.stream_openai_impl(params).await
    }

    /// Stream from GitHub Copilot API (OpenAI-compatible)
    pub async fn stream_copilot(
        &self,
        token: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let params = OpenAIStreamParams {
            api_key: token.to_string(),
            base_url: "https://api.githubcopilot.com".to_string(),
            model: model.to_string(),
            messages,
            system,
            tools,
            max_tokens,
            request_modifier: Some(Box::new(|builder| {
                builder
                    .header("editor-version", "opencode/0.1.0")
                    .header("copilot-integration-id", "vscode-chat")
            })),
        };
        self.stream_openai_impl(params).await
    }

    /// Generic OpenAI-compatible streaming implementation
    async fn stream_openai_impl(
        &self,
        params: OpenAIStreamParams,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let openai_tools: Vec<_> = params
            .tools
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

        let openai_messages =
            convert_messages_to_openai_with_system(params.messages, params.system.clone());

        let mut request_body = serde_json::json!({
            "model": params.model,
            "max_tokens": params.max_tokens,
            "messages": openai_messages,
            "tools": openai_tools,
            "stream": true,
        });

        // Add stream_options for OpenAI (not Copilot)
        if !params.base_url.contains("githubcopilot.com") {
            request_body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        // Check request body size (5MB limit)
        let request_body_str = match serde_json::to_string(&request_body) {
            Ok(s) => s,
            Err(e) => {
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    let _ = tx_clone
                        .send(StreamEvent::Error(format!(
                            "Failed to serialize request: {}",
                            e
                        )))
                        .await;
                });
                return Ok(rx);
            }
        };
        let request_size = request_body_str.len();
        const MAX_REQUEST_SIZE: usize = 5 * 1024 * 1024; // 5MB

        if request_size > MAX_REQUEST_SIZE {
            // Count tool calls to provide better guidance
            let tool_call_count = request_body
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|msgs| msgs.last())
                .and_then(|msg| msg.get("content"))
                .and_then(|content| content.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .count()
                })
                .unwrap_or(0);

            let error_msg = if tool_call_count > 10 {
                format!(
                    "Request payload too large: {} bytes (max: {} bytes). \
                    You are trying to make {} tool calls at once. \
                    \n\nIMPORTANT: Use the 'batch' tool instead! \
                    \n\nExample: \
                    \n{{\n  \"tool\": \"batch\",\n  \"parameters\": {{\n    \"tool_calls\": [\n      {{\"tool\": \"webfetch\", \"parameters\": {{...}}}},\n      {{\"tool\": \"webfetch\", \"parameters\": {{...}}}}\n    ]\n  }}\n}} \
                    \n\nThe batch tool automatically handles large numbers of tool calls efficiently.",
                    request_size, MAX_REQUEST_SIZE, tool_call_count
                )
            } else {
                format!(
                    "Request payload too large: {} bytes (max: {} bytes). \
                    This usually happens when trying to make too many tool calls at once. \
                    Please use the 'batch' tool to execute multiple tools efficiently, or break down your request into smaller steps.",
                    request_size, MAX_REQUEST_SIZE
                )
            };
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                let _ = tx_clone.send(StreamEvent::Error(error_msg)).await;
            });
            return Ok(rx);
        }

        let client = self.client.clone();
        let api_key = params.api_key;
        let base_url = params.base_url;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let is_copilot = base_url.contains("githubcopilot.com");
        let request_modifier = params.request_modifier;

        tokio::spawn(async move {
            let mut builder = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json")
                .json(&request_body);

            if let Some(modifier) = request_modifier {
                builder = modifier(builder);
            }

            let result = builder.send().await;

            match result {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string());

                        let error = if is_copilot {
                            // Enhanced error messages for GitHub Copilot
                            if error_text.contains("The requested model is not supported") {
                                format!("{}\n\nMake sure the model is enabled in your copilot settings: https://github.com/settings/copilot/features", error_text)
                            } else if status == 403 {
                                "Please reauthenticate with the copilot provider to ensure your credentials work properly with opencode-rs.".to_string()
                            } else {
                                error_text
                            }
                        } else {
                            error_text
                        };

                        let _ = tx.send(StreamEvent::Error(error)).await;
                        return;
                    }

                    Self::process_sse_stream(response, tx, OpenAIParser::new()).await;
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
