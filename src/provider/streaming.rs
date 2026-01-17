//! LLM streaming implementation for various providers.
//!
//! This module provides the `StreamingClient` for streaming responses from
//! different LLM providers (Anthropic, OpenAI, GitHub Copilot).

use anyhow::Result;
use reqwest::Client;
use tokio::sync::mpsc;

// Re-export types from stream_types for convenience
pub use super::parsers::{parse_anthropic_sse, parse_openai_sse};
pub use super::stream_types::*;

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
