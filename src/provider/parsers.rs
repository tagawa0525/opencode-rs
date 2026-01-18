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

                        // Look up the actual ID for this index
                        let id = self.index_to_id.get(&index)?.clone();

                        Some(StreamEvent::ToolCallDelta {
                            id,
                            arguments_delta: partial,
                        })
                    }
                    _ => None,
                }
            }
            "content_block_start" => {
                let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
                let content_block = parsed.get("content_block")?;
                let index = parsed.get("index")?.as_u64()? as usize;

                if content_block.get("type")?.as_str()? == "tool_use" {
                    let id = content_block.get("id")?.as_str()?.to_string();
                    let name = content_block.get("name")?.as_str()?.to_string();

                    // Store the index-to-ID mapping
                    self.index_to_id.insert(index, id.clone());

                    Some(StreamEvent::ToolCallStart { id, name })
                } else {
                    None
                }
            }
            "content_block_stop" => {
                let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
                let index = parsed.get("index")?.as_u64()? as usize;

                // Look up the actual ID and remove the mapping
                let id = self.index_to_id.remove(&index)?;

                Some(StreamEvent::ToolCallEnd { id })
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
                let index = tool_call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                // Check if this is a new tool call (has ID and name) or a delta (only arguments)
                if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                    // New tool call - store the mapping
                    self.index_to_id.insert(index, id.to_string());

                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                            return Some(StreamEvent::ToolCallStart {
                                id: id.to_string(),
                                name: name.to_string(),
                            });
                        }
                    }
                } else {
                    // Delta - look up the ID from the index
                    if let Some(id) = self.index_to_id.get(&index) {
                        if let Some(function) = tool_call.get("function") {
                            if let Some(arguments) =
                                function.get("arguments").and_then(|v| v.as_str())
                            {
                                return Some(StreamEvent::ToolCallDelta {
                                    id: id.clone(),
                                    arguments_delta: arguments.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        None
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
