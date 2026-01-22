//! SSE (Server-Sent Events) parsers for LLM providers.
//!
//! This module contains parser functions for handling streaming responses
//! from different LLM providers (Anthropic, OpenAI).

use super::stream_types::StreamEvent;
use serde_json::Value;
use std::collections::HashMap;

/// Helper to extract u64 from JSON value
fn get_u64(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}

/// Helper to extract string from JSON value
fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

/// Parse usage tokens into StreamEvent
fn parse_usage(input_key: &str, output_key: &str, usage: &Value) -> StreamEvent {
    StreamEvent::Usage {
        input_tokens: get_u64(usage, input_key),
        output_tokens: get_u64(usage, output_key),
    }
}

/// Stateful parser for Anthropic SSE streams.
#[derive(Debug, Default)]
pub struct AnthropicParser {
    index_to_id: HashMap<usize, String>,
}

impl AnthropicParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, event: &str) -> Option<StreamEvent> {
        let (event_type, data) = parse_sse_event(event)?;

        match event_type.as_str() {
            "content_block_delta" => self.parse_content_delta(&data),
            "content_block_start" => self.parse_block_start(&data),
            "content_block_stop" => self.parse_block_stop(&data),
            "message_delta" => Self::parse_message_delta(&data),
            "message_stop" => Some(StreamEvent::Done {
                finish_reason: "stop".to_string(),
            }),
            "error" => Self::parse_error(&data),
            _ => None,
        }
    }

    fn parse_content_delta(&self, data: &str) -> Option<StreamEvent> {
        let parsed: Value = serde_json::from_str(data).ok()?;
        let delta = parsed.get("delta")?;

        match get_str(delta, "type")? {
            "text_delta" => Some(StreamEvent::TextDelta(get_str(delta, "text")?.to_string())),
            "thinking_delta" => Some(StreamEvent::ReasoningDelta(
                get_str(delta, "thinking")?.to_string(),
            )),
            "input_json_delta" => {
                let index = parsed.get("index")?.as_u64()? as usize;
                Some(StreamEvent::ToolCallDelta {
                    id: self.index_to_id.get(&index)?.clone(),
                    arguments_delta: get_str(delta, "partial_json")?.to_string(),
                })
            }
            _ => None,
        }
    }

    fn parse_block_start(&mut self, data: &str) -> Option<StreamEvent> {
        let parsed: Value = serde_json::from_str(data).ok()?;
        let block = parsed.get("content_block")?;

        if get_str(block, "type")? != "tool_use" {
            return None;
        }

        let index = parsed.get("index")?.as_u64()? as usize;
        let id = get_str(block, "id")?.to_string();
        let name = get_str(block, "name")?.to_string();
        self.index_to_id.insert(index, id.clone());

        Some(StreamEvent::ToolCallStart { id, name })
    }

    fn parse_block_stop(&mut self, data: &str) -> Option<StreamEvent> {
        let parsed: Value = serde_json::from_str(data).ok()?;
        let index = parsed.get("index")?.as_u64()? as usize;
        Some(StreamEvent::ToolCallEnd {
            id: self.index_to_id.remove(&index)?,
        })
    }

    fn parse_message_delta(data: &str) -> Option<StreamEvent> {
        let parsed: Value = serde_json::from_str(data).ok()?;

        if let Some(usage) = parsed.get("usage") {
            return Some(parse_usage("input_tokens", "output_tokens", usage));
        }

        get_str(parsed.get("delta")?, "stop_reason").map(|r| StreamEvent::Done {
            finish_reason: r.to_string(),
        })
    }

    fn parse_error(data: &str) -> Option<StreamEvent> {
        let parsed: Value = serde_json::from_str(data).ok()?;
        let msg = parsed
            .get("error")
            .and_then(|e| get_str(e, "message"))
            .unwrap_or("Unknown error");
        Some(StreamEvent::Error(msg.to_string()))
    }
}

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

/// Stateful parser for OpenAI SSE streams.
#[derive(Debug, Default)]
pub struct OpenAIParser {
    index_to_id: HashMap<usize, String>,
}

impl OpenAIParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(&mut self, line: &str) -> Option<StreamEvent> {
        let data = line.strip_prefix("data: ")?;

        if data == "[DONE]" {
            return Some(StreamEvent::Done {
                finish_reason: "stop".to_string(),
            });
        }

        let parsed: Value = serde_json::from_str(data).ok()?;

        if let Some(usage) = parsed.get("usage") {
            return Some(parse_usage("prompt_tokens", "completion_tokens", usage));
        }

        self.parse_choice_delta(&parsed)
    }

    fn parse_choice_delta(&mut self, parsed: &Value) -> Option<StreamEvent> {
        let choice = parsed.get("choices")?.as_array()?.first()?;
        let delta = choice.get("delta")?;

        if let Some(reason) = get_str(choice, "finish_reason") {
            if reason != "null" {
                return Some(StreamEvent::Done {
                    finish_reason: reason.to_string(),
                });
            }
        }

        if let Some(content) = get_str(delta, "content") {
            return Some(StreamEvent::TextDelta(content.to_string()));
        }

        self.parse_tool_calls(delta)
    }

    fn parse_tool_calls(&mut self, delta: &Value) -> Option<StreamEvent> {
        for tool_call in delta.get("tool_calls")?.as_array()? {
            if let Some(event) = self.parse_tool_call(tool_call) {
                return Some(event);
            }
        }
        None
    }

    fn parse_tool_call(&mut self, tool_call: &Value) -> Option<StreamEvent> {
        let index = tool_call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        // New tool call with ID
        if let Some(id) = get_str(tool_call, "id") {
            self.index_to_id.insert(index, id.to_string());
            let name = get_str(tool_call.get("function")?, "name")?;
            return Some(StreamEvent::ToolCallStart {
                id: id.to_string(),
                name: name.to_string(),
            });
        }

        // Delta for existing tool call
        let id = self.index_to_id.get(&index)?;
        let args = get_str(tool_call.get("function")?, "arguments")?;
        Some(StreamEvent::ToolCallDelta {
            id: id.clone(),
            arguments_delta: args.to_string(),
        })
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
                Some(StreamEvent::Usage { input_tokens, output_tokens })
                    if input_tokens == 100 && output_tokens == 50
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
                Some(StreamEvent::Usage { input_tokens, output_tokens })
                    if input_tokens == 100 && output_tokens == 50
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
