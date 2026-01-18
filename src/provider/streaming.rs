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
    pub async fn stream_openai(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        self.stream_openai_compatible(api_key, base_url, model, messages, tools, max_tokens, None)
            .await
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
        self.stream_openai_compatible(
            token,
            "https://api.githubcopilot.com",
            model,
            messages,
            tools,
            max_tokens,
            Some(Box::new(|builder| {
                builder
                    .header("editor-version", "opencode/0.1.0")
                    .header("copilot-integration-id", "vscode-chat")
            })),
        )
        .await
    }

    /// Generic OpenAI-compatible streaming
    async fn stream_openai_compatible(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        max_tokens: u64,
        request_modifier: Option<
            Box<dyn FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send>,
        >,
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

        let openai_messages = convert_messages_to_openai(messages);

        let mut request_body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": openai_messages,
            "tools": openai_tools,
            "stream": true,
        });

        // Add stream_options for OpenAI (not Copilot)
        if !base_url.contains("githubcopilot.com") {
            request_body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        let client = self.client.clone();
        let api_key = api_key.to_string();
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let is_copilot = base_url.contains("githubcopilot.com");

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
