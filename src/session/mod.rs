//! Session management module.
//!
//! This module handles chat sessions, including creation, persistence,
//! message management, and session lifecycle.

mod message;
mod parts;
pub mod system;
mod types;

pub use message::*;

use crate::bus::{self, Event};
use crate::id::{self, IdPrefix};
use crate::storage;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier
    pub id: String,

    /// Human-readable slug
    pub slug: String,

    /// Project ID this session belongs to
    pub project_id: String,

    /// Working directory
    pub directory: String,

    /// Parent session ID (for child sessions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Session title
    pub title: String,

    /// Opencode version that created this session
    pub version: String,

    /// Timestamps
    pub time: SessionTime,

    /// Session share info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareInfo>,

    /// Session summary (file changes, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,

    /// Permission ruleset for this session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<serde_json::Value>,

    /// Current model for this session (cached, source of truth is message metadata)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTime {
    /// Creation timestamp (milliseconds)
    pub created: i64,

    /// Last update timestamp (milliseconds)
    pub updated: i64,

    /// Compaction start timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacting: Option<i64>,

    /// Archive timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub additions: u32,
    pub deletions: u32,
    pub files: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<FileDiff>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// Session events
#[derive(Debug, Clone)]
pub struct SessionCreated {
    pub session: Session,
}
impl Event for SessionCreated {}

#[derive(Debug, Clone)]
pub struct SessionUpdated {
    pub session: Session,
}
impl Event for SessionUpdated {}

#[derive(Debug, Clone)]
pub struct SessionDeleted {
    pub session: Session,
}
impl Event for SessionDeleted {}

impl Session {
    /// Create a new session
    pub async fn create(options: CreateSessionOptions) -> Result<Self> {
        let now = Utc::now().timestamp_millis();
        let project_id = options.project_id.unwrap_or_else(|| "default".to_string());
        let directory = options.directory.unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string()
        });

        let title = options.title.unwrap_or_else(|| {
            let prefix = if options.parent_id.is_some() {
                "Child session"
            } else {
                "New session"
            };
            format!("{} - {}", prefix, Utc::now().to_rfc3339())
        });

        let session = Session {
            id: id::descending(IdPrefix::Session),
            slug: generate_slug(),
            project_id: project_id.clone(),
            directory,
            parent_id: options.parent_id,
            title,
            version: env!("CARGO_PKG_VERSION").to_string(),
            time: SessionTime {
                created: now,
                updated: now,
                compacting: None,
                archived: None,
            },
            share: None,
            summary: None,
            permission: options.permission,
            model: None, // Will be set when first prompt is sent
        };

        // Persist to storage
        storage::global()
            .write(&["session", &project_id, &session.id], &session)
            .await
            .context("Failed to save session")?;

        // Publish event
        bus::publish(SessionCreated {
            session: session.clone(),
        })
        .await;

        Ok(session)
    }

    /// Get a session by ID
    pub async fn get(project_id: &str, session_id: &str) -> Result<Option<Self>> {
        storage::global()
            .read(&["session", project_id, session_id])
            .await
    }

    /// Update a session
    pub async fn update<F>(&mut self, project_id: &str, updater: F) -> Result<()>
    where
        F: FnOnce(&mut Session),
    {
        updater(self);
        self.time.updated = Utc::now().timestamp_millis();

        storage::global()
            .write(&["session", project_id, &self.id], self)
            .await
            .context("Failed to update session")?;

        bus::publish(SessionUpdated {
            session: self.clone(),
        })
        .await;

        Ok(())
    }

    /// Delete a session
    pub async fn delete(project_id: &str, session_id: &str) -> Result<()> {
        // Load session first for the event
        let session: Option<Session> = storage::global()
            .read(&["session", project_id, session_id])
            .await?;

        // Delete messages and parts
        let messages = storage::global().list(&["message", session_id]).await?;
        for msg_key in messages {
            // Delete parts for this message
            let message_id = msg_key.last().unwrap();
            let parts = storage::global().list(&["part", message_id]).await?;
            for part_key in parts {
                storage::global()
                    .remove(&part_key.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                    .await?;
            }
            // Delete message
            storage::global()
                .remove(&msg_key.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                .await?;
        }

        // Delete session
        storage::global()
            .remove(&["session", project_id, session_id])
            .await
            .context("Failed to delete session")?;

        if let Some(session) = session {
            bus::publish(SessionDeleted { session }).await;
        }

        Ok(())
    }

    /// List all sessions for a project
    pub async fn list(project_id: &str) -> Result<Vec<Session>> {
        let keys = storage::global().list(&["session", project_id]).await?;
        let mut sessions = Vec::new();

        for key in keys {
            if let Some(session) = storage::global()
                .read::<Session>(&key.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                .await?
            {
                sessions.push(session);
            }
        }

        // Sort by creation time (newest first due to descending IDs)
        sessions.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(sessions)
    }

    /// Get child sessions
    pub async fn children(&self, project_id: &str) -> Result<Vec<Session>> {
        let all_sessions = Self::list(project_id).await?;
        Ok(all_sessions
            .into_iter()
            .filter(|s| s.parent_id.as_ref() == Some(&self.id))
            .collect())
    }

    /// Check if title is a default title
    pub fn is_default_title(&self) -> bool {
        self.title.starts_with("New session - ") || self.title.starts_with("Child session - ")
    }

    /// Touch the session (update timestamp)
    pub async fn touch(&mut self, project_id: &str) -> Result<()> {
        self.update(project_id, |_| {}).await
    }

    /// Get all messages for this session
    pub async fn messages(&self) -> Result<Vec<Message>> {
        Message::list(&self.id).await
    }

    /// Get the current model for this session
    /// Priority: session.model > last message model
    pub async fn get_model(&self) -> Option<ModelRef> {
        // 1. Check in-memory session model cache
        if let Some(ref model) = self.model {
            return Some(model.clone());
        }

        // 2. Fall back to last user message model (source of truth)
        self.last_model().await
    }

    /// Get the last model used in message history
    async fn last_model(&self) -> Option<ModelRef> {
        let messages = match self.messages().await {
            Ok(msgs) => msgs,
            Err(_) => return None,
        };

        // Iterate in reverse to find the most recent user message with a model
        for message in messages.iter().rev() {
            if let Message::User(user_msg) = message {
                return Some(user_msg.model.clone());
            }
        }

        None
    }

    /// Set the model for this session and persist
    pub async fn set_model(&mut self, project_id: &str, model: ModelRef) -> Result<()> {
        self.model = Some(model);
        self.update(project_id, |_| {}).await
    }
}

/// Options for creating a new session
#[derive(Debug, Default)]
pub struct CreateSessionOptions {
    pub project_id: Option<String>,
    pub directory: Option<String>,
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub permission: Option<serde_json::Value>,
}

/// Generate a random slug for the session
fn generate_slug() -> String {
    use rand::Rng;
    let adjectives = [
        "quick", "lazy", "happy", "brave", "calm", "eager", "fair", "gentle", "keen", "lively",
    ];
    let nouns = [
        "fox", "dog", "cat", "bird", "fish", "bear", "wolf", "deer", "hawk", "owl",
    ];

    let mut rng = rand::rng();
    let adj = adjectives[rng.random_range(0..adjectives.len())];
    let noun = nouns[rng.random_range(0..nouns.len())];
    let num: u16 = rng.random_range(100..1000);

    format!("{}-{}-{}", adj, noun, num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let session = Session::create(CreateSessionOptions::default())
            .await
            .unwrap();

        assert!(session.id.starts_with("ses_"));
        assert!(!session.slug.is_empty());
        assert!(session.is_default_title());
    }
}
