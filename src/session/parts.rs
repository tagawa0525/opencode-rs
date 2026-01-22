//! Message part types for session conversations.
//!
//! This module defines the various part types that can be attached to messages,
//! including text, tool calls, files, and other structured content.

use crate::bus::{self, Event};
use crate::storage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::message::{MessageError, MessageTime, ModelRef, TokenUsage};

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

/// Macro to access a base field from any Part variant
macro_rules! part_base_field {
    ($self:expr, $field:ident) => {
        match $self {
            Part::Text(p) => &p.base.$field,
            Part::Reasoning(p) => &p.base.$field,
            Part::File(p) => &p.base.$field,
            Part::Tool(p) => &p.base.$field,
            Part::StepStart(p) => &p.base.$field,
            Part::StepFinish(p) => &p.base.$field,
            Part::Subtask(p) => &p.base.$field,
            Part::Compaction(p) => &p.base.$field,
            Part::Retry(p) => &p.base.$field,
            Part::Agent(p) => &p.base.$field,
            Part::Snapshot(p) => &p.base.$field,
            Part::Patch(p) => &p.base.$field,
        }
    };
}

impl Part {
    pub fn id(&self) -> &str {
        part_base_field!(self, id)
    }

    pub fn message_id(&self) -> &str {
        part_base_field!(self, message_id)
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

        bus::publish(PartUpdated {}).await;

        Ok(())
    }
}

/// Base fields for all parts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartBase {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
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
pub struct PartUpdated {}
impl Event for PartUpdated {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{self, IdPrefix};

    mod part {
        use super::*;

        #[test]
        fn test_part_id() {
            let text_part = Part::Text(TextPart {
                base: PartBase {
                    id: id::ascending(IdPrefix::Part),
                    session_id: "session_123".to_string(),
                    message_id: "msg_456".to_string(),
                },
                text: "Hello".to_string(),
                synthetic: None,
                ignored: None,
                time: None,
                metadata: None,
            });
            let tool_part = Part::Tool(ToolPart {
                base: PartBase {
                    id: id::ascending(IdPrefix::Part),
                    session_id: "session_123".to_string(),
                    message_id: "msg_456".to_string(),
                },
                tool: "bash".to_string(),
                call_id: "call_789".to_string(),
                state: ToolState::Pending(ToolStatePending {
                    input: serde_json::Value::Null,
                    raw: String::new(),
                }),
                metadata: None,
            });

            assert!(text_part.id().starts_with("prt_"));
            assert!(tool_part.id().starts_with("prt_"));
        }

        #[test]
        fn test_part_message_id() {
            let part = Part::Text(TextPart {
                base: PartBase {
                    id: id::ascending(IdPrefix::Part),
                    session_id: "session_123".to_string(),
                    message_id: "msg_456".to_string(),
                },
                text: "Hello".to_string(),
                synthetic: None,
                ignored: None,
                time: None,
                metadata: None,
            });
            assert_eq!(part.message_id(), "msg_456");
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
}
