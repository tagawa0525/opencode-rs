//! Message types for session conversations.
//!
//! This module defines the message structure, including user messages,
//! assistant messages, and various part types (text, tool calls, etc.).

use crate::bus::{self, Event};
use crate::id::{self, IdPrefix};
use crate::storage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Message role
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// Base message information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
}

impl Message {
    pub fn id(&self) -> &str {
        match self {
            Message::User(m) => &m.id,
            Message::Assistant(m) => &m.id,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Message::User(m) => &m.session_id,
            Message::Assistant(m) => &m.session_id,
        }
    }

    pub fn role(&self) -> MessageRole {
        match self {
            Message::User(_) => MessageRole::User,
            Message::Assistant(_) => MessageRole::Assistant,
        }
    }

    /// Create a new user message
    pub fn user(session_id: &str, agent: &str, model: ModelRef) -> UserMessage {
        let now = chrono::Utc::now().timestamp_millis();
        UserMessage {
            id: id::ascending(IdPrefix::Message),
            session_id: session_id.to_string(),
            time: MessageTime { created: now },
            agent: agent.to_string(),
            model,
            summary: None,
            system: None,
            tools: None,
            variant: None,
        }
    }

    /// Create a new assistant message
    pub fn assistant(
        session_id: &str,
        parent_id: &str,
        agent: &str,
        provider_id: &str,
        model_id: &str,
        path: MessagePath,
    ) -> AssistantMessage {
        let now = chrono::Utc::now().timestamp_millis();
        AssistantMessage {
            id: id::ascending(IdPrefix::Message),
            session_id: session_id.to_string(),
            parent_id: parent_id.to_string(),
            time: AssistantMessageTime {
                created: now,
                completed: None,
            },
            agent: agent.to_string(),
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            #[allow(deprecated)]
            mode: agent.to_string(), // deprecated
            path,
            error: None,
            summary: None,
            cost: 0.0,
            tokens: TokenUsage::default(),
            finish: None,
        }
    }

    /// List all messages for a session
    pub async fn list(session_id: &str) -> Result<Vec<Message>> {
        let keys = storage::global().list(&["message", session_id]).await?;
        let mut messages = Vec::new();

        for key in keys {
            if let Some(message) = storage::global()
                .read::<Message>(&key.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                .await?
            {
                messages.push(message);
            }
        }

        // Sort by ID (chronological order)
        messages.sort_by(|a, b| a.id().cmp(b.id()));

        Ok(messages)
    }

    /// Save the message
    pub async fn save(&self) -> Result<()> {
        storage::global()
            .write(&["message", self.session_id(), self.id()], self)
            .await
            .context("Failed to save message")?;

        bus::publish(MessageUpdated {
            message: self.clone(),
        })
        .await;

        Ok(())
    }

    /// Get message with its parts
    pub async fn with_parts(&self) -> Result<MessageWithParts> {
        let parts = Part::list(self.id()).await?;
        Ok(MessageWithParts {
            message: self.clone(),
            parts,
        })
    }
}

/// User message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub id: String,
    pub session_id: String,
    pub time: MessageTime,
    pub agent: String,
    pub model: ModelRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<UserSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Assistant message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub id: String,
    pub session_id: String,
    pub parent_id: String,
    pub time: AssistantMessageTime,
    pub agent: String,
    pub provider_id: String,
    pub model_id: String,
    #[deprecated]
    pub mode: String,
    pub path: MessagePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,
    pub cost: f64,
    pub tokens: TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTime {
    pub created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageTime {
    pub created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePath {
    pub cwd: String,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub diffs: Vec<super::FileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum MessageError {
    #[serde(rename = "ProviderAuthError")]
    AuthError {
        provider_id: String,
        message: String,
    },
    #[serde(rename = "MessageOutputLengthError")]
    OutputLengthError {},
    #[serde(rename = "MessageAbortedError")]
    AbortedError { message: String },
    #[serde(rename = "APIError")]
    ApiError {
        message: String,
        status_code: Option<u16>,
        is_retryable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_headers: Option<HashMap<String, String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_body: Option<String>,
    },
    #[serde(rename = "UnknownError")]
    Unknown { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache: CacheUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheUsage {
    pub read: u64,
    pub write: u64,
}

/// Message with parts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithParts {
    pub message: Message,
    pub parts: Vec<Part>,
}

/// Message events
#[derive(Debug, Clone)]
pub struct MessageUpdated {
    pub message: Message,
}
impl Event for MessageUpdated {}

#[derive(Debug, Clone)]
pub struct MessageRemoved {
    pub session_id: String,
    pub message_id: String,
}
impl Event for MessageRemoved {}

/// Message part types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Part {
    #[serde(rename = "text")]
    Text(TextPart),
    #[serde(rename = "reasoning")]
    Reasoning(ReasoningPart),
    #[serde(rename = "file")]
    File(FilePart),
    #[serde(rename = "tool")]
    Tool(ToolPart),
    #[serde(rename = "step-start")]
    StepStart(StepStartPart),
    #[serde(rename = "step-finish")]
    StepFinish(StepFinishPart),
    #[serde(rename = "subtask")]
    Subtask(SubtaskPart),
    #[serde(rename = "compaction")]
    Compaction(CompactionPart),
    #[serde(rename = "retry")]
    Retry(RetryPart),
    #[serde(rename = "agent")]
    Agent(AgentPart),
    #[serde(rename = "snapshot")]
    Snapshot(SnapshotPart),
    #[serde(rename = "patch")]
    Patch(PatchPart),
}

impl Part {
    pub fn id(&self) -> &str {
        match self {
            Part::Text(p) => &p.base.id,
            Part::Reasoning(p) => &p.base.id,
            Part::File(p) => &p.base.id,
            Part::Tool(p) => &p.base.id,
            Part::StepStart(p) => &p.base.id,
            Part::StepFinish(p) => &p.base.id,
            Part::Subtask(p) => &p.base.id,
            Part::Compaction(p) => &p.base.id,
            Part::Retry(p) => &p.base.id,
            Part::Agent(p) => &p.base.id,
            Part::Snapshot(p) => &p.base.id,
            Part::Patch(p) => &p.base.id,
        }
    }

    pub fn message_id(&self) -> &str {
        match self {
            Part::Text(p) => &p.base.message_id,
            Part::Reasoning(p) => &p.base.message_id,
            Part::File(p) => &p.base.message_id,
            Part::Tool(p) => &p.base.message_id,
            Part::StepStart(p) => &p.base.message_id,
            Part::StepFinish(p) => &p.base.message_id,
            Part::Subtask(p) => &p.base.message_id,
            Part::Compaction(p) => &p.base.message_id,
            Part::Retry(p) => &p.base.message_id,
            Part::Agent(p) => &p.base.message_id,
            Part::Snapshot(p) => &p.base.message_id,
            Part::Patch(p) => &p.base.message_id,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Part::Text(p) => &p.base.session_id,
            Part::Reasoning(p) => &p.base.session_id,
            Part::File(p) => &p.base.session_id,
            Part::Tool(p) => &p.base.session_id,
            Part::StepStart(p) => &p.base.session_id,
            Part::StepFinish(p) => &p.base.session_id,
            Part::Subtask(p) => &p.base.session_id,
            Part::Compaction(p) => &p.base.session_id,
            Part::Retry(p) => &p.base.session_id,
            Part::Agent(p) => &p.base.session_id,
            Part::Snapshot(p) => &p.base.session_id,
            Part::Patch(p) => &p.base.session_id,
        }
    }

    /// List all parts for a message
    pub async fn list(message_id: &str) -> Result<Vec<Part>> {
        let keys = storage::global().list(&["part", message_id]).await?;
        let mut parts = Vec::new();

        for key in keys {
            if let Some(part) = storage::global()
                .read::<Part>(&key.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                .await?
            {
                parts.push(part);
            }
        }

        // Sort by ID (chronological order)
        parts.sort_by(|a, b| a.id().cmp(b.id()));

        Ok(parts)
    }

    /// Save the part
    pub async fn save(&self) -> Result<()> {
        storage::global()
            .write(&["part", self.message_id(), self.id()], self)
            .await
            .context("Failed to save part")?;

        bus::publish(PartUpdated { part: self.clone() }).await;

        Ok(())
    }

    /// Create a new text part
    pub fn text(session_id: &str, message_id: &str, text: String) -> Self {
        Part::Text(TextPart {
            base: PartBase::new(session_id, message_id),
            text,
            synthetic: None,
            ignored: None,
            time: None,
            metadata: None,
        })
    }

    /// Create a new tool part
    pub fn tool(session_id: &str, message_id: &str, tool: String, call_id: String) -> Self {
        Part::Tool(ToolPart {
            base: PartBase::new(session_id, message_id),
            tool,
            call_id,
            state: ToolState::Pending(ToolStatePending {
                input: serde_json::Value::Null,
                raw: String::new(),
            }),
            metadata: None,
        })
    }
}

/// Base fields for all parts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartBase {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
}

impl PartBase {
    pub fn new(session_id: &str, message_id: &str) -> Self {
        Self {
            id: id::ascending(IdPrefix::Part),
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub text: String,
    pub time: PartTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePart {
    #[serde(flatten)]
    pub base: PartBase,
    pub mime: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<FileSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FileSource {
    #[serde(rename = "file")]
    File { path: String, text: FileSourceText },
    #[serde(rename = "symbol")]
    Symbol {
        path: String,
        text: FileSourceText,
        name: String,
        kind: i32,
    },
    #[serde(rename = "resource")]
    Resource {
        client_name: String,
        uri: String,
        text: FileSourceText,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSourceText {
    pub value: String,
    pub start: i32,
    pub end: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub tool: String,
    pub call_id: String,
    pub state: ToolState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ToolState {
    #[serde(rename = "pending")]
    Pending(ToolStatePending),
    #[serde(rename = "running")]
    Running(ToolStateRunning),
    #[serde(rename = "completed")]
    Completed(ToolStateCompleted),
    #[serde(rename = "error")]
    Error(ToolStateError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatePending {
    pub input: serde_json::Value,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateRunning {
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub time: ToolTimeStart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateCompleted {
    pub input: serde_json::Value,
    pub output: String,
    pub title: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub time: ToolTimeComplete,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FilePart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateError {
    pub input: serde_json::Value,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub time: ToolTimeComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTimeStart {
    pub start: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTimeComplete {
    pub start: i64,
    pub end: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStartPart {
    #[serde(flatten)]
    pub base: PartBase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepFinishPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub reason: String,
    pub cost: f64,
    pub tokens: TokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub prompt: String,
    pub description: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub attempt: u32,
    pub error: MessageError,
    pub time: MessageTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<FileSourceText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchPart {
    #[serde(flatten)]
    pub base: PartBase,
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartTime {
    pub start: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

/// Part events
#[derive(Debug, Clone)]
pub struct PartUpdated {
    pub part: Part,
}
impl Event for PartUpdated {}

#[derive(Debug, Clone)]
pub struct PartRemoved {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
}
impl Event for PartRemoved {}

#[cfg(test)]
mod tests {
    use super::*;

    mod message_role {
        use super::*;

        #[test]
        fn test_serialize_user() {
            let role = MessageRole::User;
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, r#""user""#);
        }

        #[test]
        fn test_serialize_assistant() {
            let role = MessageRole::Assistant;
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, r#""assistant""#);
        }

        #[test]
        fn test_deserialize_user() {
            let role: MessageRole = serde_json::from_str(r#""user""#).unwrap();
            assert_eq!(role, MessageRole::User);
        }

        #[test]
        fn test_deserialize_assistant() {
            let role: MessageRole = serde_json::from_str(r#""assistant""#).unwrap();
            assert_eq!(role, MessageRole::Assistant);
        }
    }

    mod message {
        use super::*;

        #[test]
        fn test_user_message_creation() {
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let msg = Message::user("session_123", "default", model);

            assert!(msg.id.starts_with("msg_"));
            assert_eq!(msg.session_id, "session_123");
            assert_eq!(msg.agent, "default");
            assert_eq!(msg.model.provider_id, "anthropic");
        }

        #[test]
        fn test_assistant_message_creation() {
            let path = MessagePath {
                cwd: "/home/user".to_string(),
                root: "/home/user/project".to_string(),
            };
            let msg = Message::assistant(
                "session_123",
                "msg_parent",
                "default",
                "anthropic",
                "claude-3-5-sonnet",
                path,
            );

            assert!(msg.id.starts_with("msg_"));
            assert_eq!(msg.session_id, "session_123");
            assert_eq!(msg.parent_id, "msg_parent");
            assert_eq!(msg.provider_id, "anthropic");
            assert_eq!(msg.model_id, "claude-3-5-sonnet");
        }

        #[test]
        fn test_message_id() {
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let user_msg = Message::User(Message::user("session_123", "default", model.clone()));

            let path = MessagePath {
                cwd: "/home/user".to_string(),
                root: "/home/user/project".to_string(),
            };
            let assistant_msg = Message::Assistant(Message::assistant(
                "session_123",
                "msg_parent",
                "default",
                "anthropic",
                "claude-3-5-sonnet",
                path,
            ));

            assert!(user_msg.id().starts_with("msg_"));
            assert!(assistant_msg.id().starts_with("msg_"));
        }

        #[test]
        fn test_message_role() {
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let user_msg = Message::User(Message::user("session_123", "default", model));

            assert_eq!(user_msg.role(), MessageRole::User);
        }

        #[test]
        fn test_message_serialize_deserialize() {
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let user_msg = Message::User(Message::user("session_123", "default", model));

            let json = serde_json::to_string(&user_msg).unwrap();
            let parsed: Message = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed.id(), user_msg.id());
            assert_eq!(parsed.session_id(), user_msg.session_id());
        }
    }

    mod part {
        use super::*;

        #[test]
        fn test_text_part_creation() {
            let part = Part::text("session_123", "msg_456", "Hello world".to_string());

            match part {
                Part::Text(text_part) => {
                    assert!(text_part.base.id.starts_with("prt_"));
                    assert_eq!(text_part.base.session_id, "session_123");
                    assert_eq!(text_part.base.message_id, "msg_456");
                    assert_eq!(text_part.text, "Hello world");
                }
                _ => panic!("Expected Text part"),
            }
        }

        #[test]
        fn test_tool_part_creation() {
            let part = Part::tool(
                "session_123",
                "msg_456",
                "bash".to_string(),
                "call_789".to_string(),
            );

            match part {
                Part::Tool(tool_part) => {
                    assert!(tool_part.base.id.starts_with("prt_"));
                    assert_eq!(tool_part.tool, "bash");
                    assert_eq!(tool_part.call_id, "call_789");
                    assert!(matches!(tool_part.state, ToolState::Pending(_)));
                }
                _ => panic!("Expected Tool part"),
            }
        }

        #[test]
        fn test_part_id() {
            let text_part = Part::text("session_123", "msg_456", "Hello".to_string());
            let tool_part = Part::tool(
                "session_123",
                "msg_456",
                "bash".to_string(),
                "call_789".to_string(),
            );

            assert!(text_part.id().starts_with("prt_"));
            assert!(tool_part.id().starts_with("prt_"));
        }

        #[test]
        fn test_part_message_id() {
            let part = Part::text("session_123", "msg_456", "Hello".to_string());
            assert_eq!(part.message_id(), "msg_456");
        }

        #[test]
        fn test_part_session_id() {
            let part = Part::text("session_123", "msg_456", "Hello".to_string());
            assert_eq!(part.session_id(), "session_123");
        }
    }

    mod tool_state {
        use super::*;

        #[test]
        fn test_pending_state_serialize() {
            let state = ToolState::Pending(ToolStatePending {
                input: serde_json::json!({"cmd": "ls"}),
                raw: r#"{"cmd": "ls"}"#.to_string(),
            });

            let json = serde_json::to_string(&state).unwrap();
            assert!(json.contains(r#""status":"pending""#));
        }

        #[test]
        fn test_running_state_serialize() {
            let state = ToolState::Running(ToolStateRunning {
                input: serde_json::json!({"cmd": "ls"}),
                title: Some("Running bash".to_string()),
                metadata: None,
                time: ToolTimeStart { start: 1000 },
            });

            let json = serde_json::to_string(&state).unwrap();
            assert!(json.contains(r#""status":"running""#));
        }

        #[test]
        fn test_completed_state_serialize() {
            let state = ToolState::Completed(ToolStateCompleted {
                input: serde_json::json!({"cmd": "ls"}),
                output: "file.txt".to_string(),
                title: "Listed files".to_string(),
                metadata: std::collections::HashMap::new(),
                time: ToolTimeComplete {
                    start: 1000,
                    end: 2000,
                    compacted: None,
                },
                attachments: None,
            });

            let json = serde_json::to_string(&state).unwrap();
            assert!(json.contains(r#""status":"completed""#));
        }

        #[test]
        fn test_error_state_serialize() {
            let state = ToolState::Error(ToolStateError {
                input: serde_json::json!({"cmd": "rm -rf /"}),
                error: "Permission denied".to_string(),
                metadata: None,
                time: ToolTimeComplete {
                    start: 1000,
                    end: 2000,
                    compacted: None,
                },
            });

            let json = serde_json::to_string(&state).unwrap();
            assert!(json.contains(r#""status":"error""#));
        }
    }

    mod message_error {
        use super::*;

        #[test]
        fn test_auth_error_serialize() {
            let error = MessageError::AuthError {
                provider_id: "anthropic".to_string(),
                message: "Invalid API key".to_string(),
            };

            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"ProviderAuthError""#));
        }

        #[test]
        fn test_api_error_serialize() {
            let error = MessageError::ApiError {
                message: "Rate limit exceeded".to_string(),
                status_code: Some(429),
                is_retryable: true,
                response_headers: None,
                response_body: None,
            };

            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"APIError""#));
            assert!(json.contains(r#""status_code":429"#));
        }

        #[test]
        fn test_output_length_error_serialize() {
            let error = MessageError::OutputLengthError {};
            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"MessageOutputLengthError""#));
        }

        #[test]
        fn test_aborted_error_serialize() {
            let error = MessageError::AbortedError {
                message: "User cancelled".to_string(),
            };
            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"MessageAbortedError""#));
        }
    }

    mod token_usage {
        use super::*;

        #[test]
        fn test_default() {
            let usage = TokenUsage::default();
            assert_eq!(usage.input, 0);
            assert_eq!(usage.output, 0);
            assert_eq!(usage.reasoning, 0);
            assert_eq!(usage.cache.read, 0);
            assert_eq!(usage.cache.write, 0);
        }

        #[test]
        fn test_serialize_deserialize() {
            let usage = TokenUsage {
                input: 100,
                output: 50,
                reasoning: 10,
                cache: CacheUsage { read: 5, write: 3 },
            };

            let json = serde_json::to_string(&usage).unwrap();
            let parsed: TokenUsage = serde_json::from_str(&json).unwrap();

            assert_eq!(parsed.input, 100);
            assert_eq!(parsed.output, 50);
            assert_eq!(parsed.cache.read, 5);
        }
    }
}
