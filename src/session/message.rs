//! Message types for session conversations.
//!
//! This module defines the message structure, including user messages
//! and assistant messages. Part types are defined in parts.rs.

use crate::bus::{self, Event};
use crate::id::{self, IdPrefix};
use crate::storage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// Re-export Part types from parts module
pub use super::parts::*;

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
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<u32>,
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
    pub path: MessagePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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
    /// Text content summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Files attached
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u32>,
}

/// Message errors
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum MessageError {
    #[serde(rename = "ProviderAuthError")]
    Auth {
        provider_id: String,
        message: String,
    },
    #[serde(rename = "APIError")]
    Api {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status_code: Option<u16>,
        is_retryable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_headers: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_body: Option<String>,
    },
    #[serde(rename = "MessageOutputLengthError")]
    OutputLength {},
    #[serde(rename = "MessageAbortedError")]
    Aborted { message: String },
}

/// Token usage statistics
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

/// Message with its parts
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

    mod message_error {
        use super::*;

        #[test]
        fn test_auth_error_serialize() {
            let error = MessageError::Auth {
                provider_id: "anthropic".to_string(),
                message: "Invalid API key".to_string(),
            };

            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"ProviderAuthError""#));
        }

        #[test]
        fn test_api_error_serialize() {
            let error = MessageError::Api {
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
            let error = MessageError::OutputLength {};
            let json = serde_json::to_string(&error).unwrap();
            assert!(json.contains(r#""name":"MessageOutputLengthError""#));
        }

        #[test]
        fn test_aborted_error_serialize() {
            let error = MessageError::Aborted {
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
