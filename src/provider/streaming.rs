//! LLM streaming implementation for various providers.
//!
//! This module provides the `StreamingClient` for streaming responses from
//! different LLM providers (Anthropic, OpenAI, GitHub Copilot).

use anyhow::Result;
use futures::StreamExt;
use reqwest::{Client, Response};
use tokio::sync::mpsc;

pub use super::parsers::{AnthropicParser, OpenAIParser};
pub use super::stream_types::*;

/// Request parameters for OpenAI-compatible API calls
#[derive(Debug, Clone)]
pub struct OpenAIRequest {
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u64,
}

type RequestModifier =
    Option<Box<dyn FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder + Send>>;

/// Internal parameters for OpenAI-compatible streaming
struct OpenAIParams {
    api_key: String,
    base_url: String,
    model: String,
    messages: Vec<ChatMessage>,
    system: Option<String>,
    tools: Vec<ToolDefinition>,
    max_tokens: u64,
    request_modifier: RequestModifier,
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
        "\n\n"
    }
}

impl SseParser for OpenAIParser {
    fn parse(&mut self, chunk: &str) -> Option<StreamEvent> {
        self.parse(chunk)
    }

    fn event_delimiter(&self) -> &str {
        "\n"
    }
}

/// Maximum request size limit (5MB)
const MAX_REQUEST_SIZE: usize = 5 * 1024 * 1024;

/// Check request size and return error message if too large
fn check_request_size(request_body: &serde_json::Value) -> Option<String> {
    let request_str = serde_json::to_string(request_body).ok()?;
    let size = request_str.len();

    if size <= MAX_REQUEST_SIZE {
        return None;
    }

    let tool_count = count_tool_calls(request_body);
    Some(build_size_error(size, tool_count))
}

fn count_tool_calls(body: &serde_json::Value) -> usize {
    body.get("messages")
        .and_then(|m| m.as_array())
        .and_then(|msgs| msgs.last())
        .and_then(|msg| msg.get("content"))
        .and_then(|c| c.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                .count()
        })
        .unwrap_or(0)
}

fn build_size_error(size: usize, tool_count: usize) -> String {
    let base = format!(
        "Request payload too large: {} bytes (max: {} bytes).",
        size, MAX_REQUEST_SIZE
    );

    if tool_count > 10 {
        format!(
            "{} You are trying to make {} tool calls at once.\n\n\
            IMPORTANT: Use the 'batch' tool instead!\n\n\
            Example:\n{{\n  \"tool\": \"batch\",\n  \"parameters\": {{\n    \"tool_calls\": [\n      \
            {{\"tool\": \"webfetch\", \"parameters\": {{...}}}},\n      \
            {{\"tool\": \"webfetch\", \"parameters\": {{...}}}}\n    ]\n  }}\n}}\n\n\
            The batch tool automatically handles large numbers of tool calls efficiently.",
            base, tool_count
        )
    } else {
        format!(
            "{} This usually happens when trying to make too many tool calls at once. \
            Please use the 'batch' tool to execute multiple tools efficiently, \
            or break down your request into smaller steps.",
            base
        )
    }
}

/// Send error through channel asynchronously
fn spawn_error(tx: mpsc::Sender<StreamEvent>, error: String) {
    tokio::spawn(async move {
        let _ = tx.send(StreamEvent::Error(error)).await;
    });
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

    /// Handle HTTP response with optional error customization
    async fn handle_response<P: SseParser>(
        result: Result<Response, reqwest::Error>,
        tx: mpsc::Sender<StreamEvent>,
        parser: P,
        error_handler: Option<fn(u16, &str) -> String>,
    ) {
        match result {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status().as_u16();
                    let text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    let error = error_handler.map_or(text.clone(), |h| h(status, &text));
                    let _ = tx.send(StreamEvent::Error(error)).await;
                    return;
                }
                Self::process_sse_stream(response, tx, parser).await;
            }
            Err(e) => {
                let _ = tx.send(StreamEvent::Error(e.to_string())).await;
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
            "tools": tools.iter().map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })).collect::<Vec<_>>(),
            "stream": true,
        });

        if let Some(error) = check_request_size(&request_body) {
            spawn_error(tx, error);
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

            Self::handle_response(result, tx, AnthropicParser::new(), None).await;
        });

        Ok(rx)
    }

    /// Stream from OpenAI-compatible API
    pub async fn stream_openai(
        &self,
        api_key: &str,
        base_url: &str,
        model: &str,
        request: OpenAIRequest,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        self.stream_openai_impl(OpenAIParams {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            messages: request.messages,
            system: request.system,
            tools: request.tools,
            max_tokens: request.max_tokens,
            request_modifier: None,
        })
        .await
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
        self.stream_openai_impl(OpenAIParams {
            api_key: token.to_string(),
            base_url: "https://api.githubcopilot.com".to_string(),
            model: model.to_string(),
            messages,
            system,
            tools,
            max_tokens,
            request_modifier: Some(Box::new(|b| {
                b.header("editor-version", "opencode/0.1.0")
                    .header("copilot-integration-id", "vscode-chat")
            })),
        })
        .await
    }

    /// Generic OpenAI-compatible streaming implementation
    async fn stream_openai_impl(
        &self,
        params: OpenAIParams,
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
            convert_messages_to_openai_with_system(params.messages, params.system);
        let is_copilot = params.base_url.contains("githubcopilot.com");

        let mut request_body = serde_json::json!({
            "model": params.model,
            "max_tokens": params.max_tokens,
            "messages": openai_messages,
            "tools": openai_tools,
            "stream": true,
        });

        if !is_copilot {
            request_body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        if let Some(error) = check_request_size(&request_body) {
            spawn_error(tx, error);
            return Ok(rx);
        }

        let client = self.client.clone();
        let url = format!("{}/chat/completions", params.base_url.trim_end_matches('/'));

        tokio::spawn(async move {
            let mut builder = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", params.api_key))
                .header("content-type", "application/json")
                .json(&request_body);

            if let Some(modifier) = params.request_modifier {
                builder = modifier(builder);
            }

            let error_handler: Option<fn(u16, &str) -> String> = if is_copilot {
                Some(copilot_error_handler)
            } else {
                None
            };

            Self::handle_response(builder.send().await, tx, OpenAIParser::new(), error_handler)
                .await;
        });

        Ok(rx)
    }
}

fn copilot_error_handler(status: u16, text: &str) -> String {
    if text.contains("The requested model is not supported") {
        format!(
            "{}\n\nMake sure the model is enabled in your copilot settings: \
            https://github.com/settings/copilot/features",
            text
        )
    } else if status == 403 {
        "Please reauthenticate with the copilot provider to ensure your credentials \
        work properly with opencode-rs."
            .to_string()
    } else {
        text.to_string()
    }
}

impl Default for StreamingClient {
    fn default() -> Self {
        Self::new()
    }
}
