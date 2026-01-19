//! SSE (Server-Sent Events) parsers for LLM providers.
//!
//! This module contains parser functions for handling streaming responses
//! from different LLM providers (Anthropic, OpenAI).

use super::stream_types::StreamEvent;
use std::collections::HashMap;

/// Stateful parser for Anthropic SSE streams.
/// Maintains index-to-ID mapping for tool calls.
#[derive(Debug, Default)]
pub struct AnthropicParser {
    /// Maps content block index to tool call ID
    index_to_id: HashMap<usize, String>,
}

impl AnthropicParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a single Anthropic SSE event with state tracking
    pub fn parse(&mut self, event: &str) -> Option<StreamEvent> {
        let (event_type, data) = parse_sse_event(event)?;

        match event_type.as_str() {
            "content_block_delta" => self.parse_content_block_delta(&data),
            "content_block_start" => self.parse_content_block_start(&data),
            "content_block_stop" => self.parse_content_block_stop(&data),
            "message_delta" => Self::parse_message_delta(&data),
            "message_stop" => Some(StreamEvent::Done {
                finish_reason: "stop".to_string(),
            }),
            "error" => Self::parse_error(&data),
            _ => None,
        }
    }

    /// Parse content_block_delta event
    fn parse_content_block_delta(&self, data: &str) -> Option<StreamEvent> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;
        let delta = parsed.get("delta")?;
        let delta_type = delta.get("type")?.as_str()?;

        match delta_type {
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
                let id = self.index_to_id.get(&index)?.clone();
                Some(StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta: partial,
                })
            }
            _ => None,
        }
    }

    /// Parse content_block_start event
    fn parse_content_block_start(&mut self, data: &str) -> Option<StreamEvent> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;
        let content_block = parsed.get("content_block")?;
        let index = parsed.get("index")?.as_u64()? as usize;

        if content_block.get("type")?.as_str()? != "tool_use" {
            return None;
        }

        let id = content_block.get("id")?.as_str()?.to_string();
        let name = content_block.get("name")?.as_str()?.to_string();
        self.index_to_id.insert(index, id.clone());

        Some(StreamEvent::ToolCallStart { id, name })
    }

    /// Parse content_block_stop event
    fn parse_content_block_stop(&mut self, data: &str) -> Option<StreamEvent> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;
        let index = parsed.get("index")?.as_u64()? as usize;
        let id = self.index_to_id.remove(&index)?;
        Some(StreamEvent::ToolCallEnd { id })
    }

    /// Parse message_delta event
    fn parse_message_delta(data: &str) -> Option<StreamEvent> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

        // Check for usage first
        if let Some(usage) = parsed.get("usage") {
            return Some(parse_anthropic_usage(usage));
        }

        // Check for stop reason
        parsed
            .get("delta")?
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|stop_reason| StreamEvent::Done {
                finish_reason: stop_reason.to_string(),
            })
    }

    /// Parse error event
    fn parse_error(data: &str) -> Option<StreamEvent> {
        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;
        let message = parsed
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error")
            .to_string();
        Some(StreamEvent::Error(message))
    }
}

/// Parse SSE event into (event_type, data)
fn parse_sse_event(event: &str) -> Option<(String, String)> {
    let mut event_type = None;
    let mut data = None;

    for line in event.lines() {
        if let Some(t) = line.strip_prefix("event: ") {
            event_type = Some(t.to_string());
        } else if let Some(d) = line.strip_prefix("data: ") {
            data = Some(d.to_string());
        }
    }

    Some((event_type?, data?))
}

/// Parse Anthropic usage data
fn parse_anthropic_usage(usage: &serde_json::Value) -> StreamEvent {
    StreamEvent::Usage {
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
    }
}

/// Stateful parser for OpenAI SSE streams.
/// Maintains index-to-ID mapping for tool calls.
#[derive(Debug, Default)]
pub struct OpenAIParser {
    /// Maps tool call index to tool call ID
    index_to_id: HashMap<usize, String>,
}

impl OpenAIParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a single OpenAI SSE line with state tracking
    pub fn parse(&mut self, line: &str) -> Option<StreamEvent> {
        let data = line.strip_prefix("data: ")?;

        if data == "[DONE]" {
            return Some(StreamEvent::Done {
                finish_reason: "stop".to_string(),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(data).ok()?;

        // Check for usage first
        if let Some(usage) = parsed.get("usage") {
            return Some(parse_openai_usage(usage));
        }

        // Parse choice delta
        self.parse_choice_delta(&parsed)
    }

    /// Parse delta from choices array
    fn parse_choice_delta(&mut self, parsed: &serde_json::Value) -> Option<StreamEvent> {
        let choice = parsed.get("choices")?.as_array()?.first()?;
        let delta = choice.get("delta")?;

        // Check for finish reason
        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if reason != "null" {
                return Some(StreamEvent::Done {
                    finish_reason: reason.to_string(),
                });
            }
        }

        // Check for content
        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
            return Some(StreamEvent::TextDelta(content.to_string()));
        }

        // Check for tool calls
        self.parse_tool_calls(delta)
    }

    /// Parse tool calls from delta
    fn parse_tool_calls(&mut self, delta: &serde_json::Value) -> Option<StreamEvent> {
        let tool_calls = delta.get("tool_calls")?.as_array()?;

        for tool_call in tool_calls {
            if let Some(event) = self.parse_single_tool_call(tool_call) {
                return Some(event);
            }
        }
        None
    }

    /// Parse a single tool call entry
    fn parse_single_tool_call(&mut self, tool_call: &serde_json::Value) -> Option<StreamEvent> {
        let index = tool_call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        // New tool call (has ID)
        if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
            self.index_to_id.insert(index, id.to_string());

            let name = tool_call
                .get("function")?
                .get("name")
                .and_then(|v| v.as_str())?;

            return Some(StreamEvent::ToolCallStart {
                id: id.to_string(),
                name: name.to_string(),
            });
        }

        // Tool call delta (arguments only)
        let id = self.index_to_id.get(&index)?;
        let arguments = tool_call
            .get("function")?
            .get("arguments")
            .and_then(|v| v.as_str())?;

        Some(StreamEvent::ToolCallDelta {
            id: id.clone(),
            arguments_delta: arguments.to_string(),
        })
    }
}

/// Parse OpenAI usage data
fn parse_openai_usage(usage: &serde_json::Value) -> StreamEvent {
    StreamEvent::Usage {
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
    }
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

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(matches!(result, Some(StreamEvent::TextDelta(text)) if text == "Hello"));
        }

        #[test]
        fn test_thinking_delta() {
            let event = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(
                matches!(result, Some(StreamEvent::ReasoningDelta(text)) if text == "Let me think...")
            );
        }

        #[test]
        fn test_tool_use_start() {
            let event = r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_123","name":"bash"}}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallStart { id, name })
                    if id == "tool_123" && name == "bash"
            ));
        }

        #[test]
        fn test_input_json_delta() {
            // First, send tool_use_start to register the ID
            let start_event = r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_123","name":"bash"}}"#;

            let delta_event = r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\":"}}"#;

            let mut parser = AnthropicParser::new();
            parser.parse(start_event); // Register the ID
            let result = parser.parse(delta_event);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallDelta { id, arguments_delta })
                    if id == "tool_123" && arguments_delta == "{\"cmd\":"
            ));
        }

        #[test]
        fn test_message_stop() {
            let event = r#"event: message_stop
data: {}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_message_delta_with_stop_reason() {
            let event = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "end_turn"
            ));
        }

        #[test]
        fn test_usage() {
            let event = r#"event: message_delta
data: {"type":"message_delta","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
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

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(matches!(
                result,
                Some(StreamEvent::Error(msg)) if msg == "Rate limit exceeded"
            ));
        }

        #[test]
        fn test_unknown_event() {
            let event = r#"event: ping
data: {}"#;

            let mut parser = AnthropicParser::new();
            let result = parser.parse(event);
            assert!(result.is_none());
        }
    }

    mod parse_openai_sse {
        use super::*;

        #[test]
        fn test_done() {
            let line = "data: [DONE]";
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_text_delta() {
            let line = r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(matches!(result, Some(StreamEvent::TextDelta(text)) if text == "Hello"));
        }

        #[test]
        fn test_finish_reason() {
            let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Done { finish_reason }) if finish_reason == "stop"
            ));
        }

        #[test]
        fn test_tool_call_start() {
            let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_abc123","index":0,"function":{"name":"bash"}}]},"index":0}]}"#;
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallStart { id, name })
                    if id == "call_abc123" && name == "bash"
            ));
        }

        #[test]
        fn test_tool_call_arguments() {
            // First, register the tool call with ID
            let start_line = r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_abc123","index":0,"function":{"name":"bash","arguments":""}}]},"index":0}]}"#;
            let args_line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"cmd\":"}}]},"index":0}]}"#;

            let mut parser = OpenAIParser::new();
            parser.parse(start_line); // Register the ID
            let result = parser.parse(args_line);
            assert!(matches!(
                result,
                Some(StreamEvent::ToolCallDelta { id, arguments_delta })
                    if id == "call_abc123" && arguments_delta == "{\"cmd\":"
            ));
        }

        #[test]
        fn test_usage() {
            let line = r#"data: {"usage":{"prompt_tokens":100,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":10}}}"#;
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(matches!(
                result,
                Some(StreamEvent::Usage { input_tokens, output_tokens, cache_read_tokens, .. })
                    if input_tokens == 100 && output_tokens == 50 && cache_read_tokens == 10
            ));
        }

        #[test]
        fn test_invalid_json() {
            let line = "data: not-json";
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(result.is_none());
        }

        #[test]
        fn test_no_data_prefix() {
            let line = "not a data line";
            let mut parser = OpenAIParser::new();
            let result = parser.parse(line);
            assert!(result.is_none());
        }
    }
}
