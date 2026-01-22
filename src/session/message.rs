//! Message types for session conversations.
//!
//! This module defines the message structure, including user messages
//! and assistant messages. Part types are defined in parts.rs.

use crate::bus::{self, Event};
#[cfg(test)]
use crate::id::{self, IdPrefix};
use crate::storage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// Re-export Part types from parts module
pub use super::parts::*;

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

        bus::publish(MessageUpdated {}).await;

        Ok(())
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

/// Message events
#[derive(Debug, Clone)]
pub struct MessageUpdated {}

impl Event for MessageUpdated {}

#[cfg(test)]
mod tests {
    use super::*;

    mod message {
        use super::*;

        #[test]
        fn test_user_message_creation() {
            let now = chrono::Utc::now().timestamp_millis();
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let msg = UserMessage {
                id: id::ascending(IdPrefix::Message),
                session_id: "session_123".to_string(),
                time: MessageTime { created: now },
                agent: "default".to_string(),
                model,
                summary: None,
                system: None,
                tools: None,
                variant: None,
            };

            assert!(msg.id.starts_with("msg_"));
            assert_eq!(msg.session_id, "session_123");
            assert_eq!(msg.agent, "default");
            assert_eq!(msg.model.provider_id, "anthropic");
        }

        #[test]
        fn test_assistant_message_creation() {
            let now = chrono::Utc::now().timestamp_millis();
            let path = MessagePath {
                cwd: "/home/user".to_string(),
                root: "/home/user/project".to_string(),
            };
            let msg = AssistantMessage {
                id: id::ascending(IdPrefix::Message),
                session_id: "session_123".to_string(),
                parent_id: "msg_parent".to_string(),
                time: AssistantMessageTime {
                    created: now,
                    completed: None,
                },
                agent: "default".to_string(),
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
                path,
                error: None,
                summary: None,
                cost: 0.0,
                tokens: TokenUsage::default(),
                finish: None,
            };

            assert!(msg.id.starts_with("msg_"));
            assert_eq!(msg.session_id, "session_123");
            assert_eq!(msg.parent_id, "msg_parent");
            assert_eq!(msg.provider_id, "anthropic");
            assert_eq!(msg.model_id, "claude-3-5-sonnet");
        }

        #[test]
        fn test_message_id() {
            let now = chrono::Utc::now().timestamp_millis();
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let user_msg = Message::User(UserMessage {
                id: id::ascending(IdPrefix::Message),
                session_id: "session_123".to_string(),
                time: MessageTime { created: now },
                agent: "default".to_string(),
                model: model.clone(),
                summary: None,
                system: None,
                tools: None,
                variant: None,
            });

            let path = MessagePath {
                cwd: "/home/user".to_string(),
                root: "/home/user/project".to_string(),
            };
            let assistant_msg = Message::Assistant(AssistantMessage {
                id: id::ascending(IdPrefix::Message),
                session_id: "session_123".to_string(),
                parent_id: "msg_parent".to_string(),
                time: AssistantMessageTime {
                    created: now,
                    completed: None,
                },
                agent: "default".to_string(),
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
                path,
                error: None,
                summary: None,
                cost: 0.0,
                tokens: TokenUsage::default(),
                finish: None,
            });

            assert!(user_msg.id().starts_with("msg_"));
            assert!(assistant_msg.id().starts_with("msg_"));
        }

        #[test]
        fn test_message_serialize_deserialize() {
            let now = chrono::Utc::now().timestamp_millis();
            let model = ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            };
            let user_msg = Message::User(UserMessage {
                id: id::ascending(IdPrefix::Message),
                session_id: "session_123".to_string(),
                time: MessageTime { created: now },
                agent: "default".to_string(),
                model,
                summary: None,
                system: None,
                tools: None,
                variant: None,
            });

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
