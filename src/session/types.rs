//! Additional session-related types.

use serde::{Deserialize, Serialize};

/// Session status during prompt processing
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is idle, ready for input
    #[default]
    Idle,
    /// Processing user input
    Processing,
    /// Waiting for tool execution
    Tool,
    /// Compacting context
    Compacting,
    /// Session has an error
    Error,
}

/// Session statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total cost in dollars
    pub total_cost: f64,
    /// Total input tokens
    pub total_input_tokens: u64,
    /// Total output tokens
    pub total_output_tokens: u64,
    /// Total reasoning tokens
    pub total_reasoning_tokens: u64,
    /// Total cache read tokens
    pub total_cache_read: u64,
    /// Total cache write tokens
    pub total_cache_write: u64,
    /// Number of messages
    pub message_count: u64,
    /// Number of tool calls
    pub tool_call_count: u64,
}

impl SessionStats {
    /// Calculate stats from messages
    pub fn from_messages(messages: &[super::Message]) -> Self {
        let mut stats = SessionStats::default();

        for message in messages {
            stats.message_count += 1;

            if let super::Message::Assistant(assistant) = message {
                stats.total_cost += assistant.cost;
                stats.total_input_tokens += assistant.tokens.input;
                stats.total_output_tokens += assistant.tokens.output;
                stats.total_reasoning_tokens += assistant.tokens.reasoning;
                stats.total_cache_read += assistant.tokens.cache.read;
                stats.total_cache_write += assistant.tokens.cache.write;
            }
        }

        stats
    }
}

/// Prompt input for a session
#[derive(Debug, Clone)]
pub struct PromptInput {
    /// The text prompt
    pub text: String,
    /// Files to attach
    pub files: Vec<PromptFile>,
    /// Agent to use (optional, uses default if not specified)
    pub agent: Option<String>,
    /// Model override
    pub model: Option<ModelOverride>,
}

impl PromptInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            files: Vec::new(),
            agent: None,
            model: None,
        }
    }

    pub fn with_files(mut self, files: Vec<PromptFile>) -> Self {
        self.files = files;
        self
    }

    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    pub fn with_model(
        mut self,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        self.model = Some(ModelOverride {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        });
        self
    }
}

/// File attached to a prompt
#[derive(Debug, Clone)]
pub struct PromptFile {
    /// File path
    pub path: String,
    /// MIME type
    pub mime_type: String,
    /// File content (for inline files)
    pub content: Option<Vec<u8>>,
}

/// Model override for a prompt
#[derive(Debug, Clone)]
pub struct ModelOverride {
    pub provider_id: String,
    pub model_id: String,
}

/// Usage information for cost calculation
#[derive(Debug, Clone, Default)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
}

impl UsageInfo {
    /// Calculate cost based on model pricing
    pub fn calculate_cost(&self, cost: &super::super::provider::ModelCost) -> f64 {
        let input_cost = (self.input_tokens as f64) * cost.input / 1_000_000.0;
        let output_cost = (self.output_tokens as f64) * cost.output / 1_000_000.0;
        let reasoning_cost = (self.reasoning_tokens as f64) * cost.output / 1_000_000.0;
        let cache_read_cost = (self.cached_input_tokens as f64) * cost.cache_read / 1_000_000.0;
        let cache_write_cost = (self.cache_write_tokens as f64) * cost.cache_write / 1_000_000.0;

        input_cost + output_cost + reasoning_cost + cache_read_cost + cache_write_cost
    }
}
