use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod builtin;
pub mod parser;
pub mod registry;
pub mod template;

/// Context provided to slash commands when executed
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub session_id: String,
    pub cwd: String,
    pub root: String,
    pub extra: HashMap<String, serde_json::Value>,
}

/// Output from a slash command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    /// The text to display to the user
    pub text: String,
    /// Whether to submit this as a prompt to the LLM
    pub submit_to_llm: bool,
    /// Optional system message to include
    pub system: Option<String>,
    /// Optional agent to use
    pub agent: Option<String>,
    /// Optional model to use
    pub model: Option<String>,
}

impl CommandOutput {
    /// Create a simple text output that doesn't submit to LLM
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            submit_to_llm: false,
            system: None,
            agent: None,
            model: None,
        }
    }

    /// Create output that submits a prompt to the LLM
    pub fn prompt(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            submit_to_llm: true,
            system: None,
            agent: None,
            model: None,
        }
    }

    /// Add a system message
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the agent to use
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// Set the model to use
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// Trait for slash commands
#[async_trait]
pub trait SlashCommand: Send + Sync {
    /// The name of the command (without the leading slash)
    fn name(&self) -> &str;

    /// A short description of what the command does
    fn description(&self) -> &str;

    /// Usage information for the command
    fn usage(&self) -> &str {
        self.name()
    }

    /// Optional aliases for the command
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// Execute the command with the given arguments
    async fn execute(&self, args: &str, ctx: &CommandContext) -> Result<CommandOutput>;

    /// Provide autocomplete suggestions for the given partial input
    async fn complete(&self, _partial: &str) -> Vec<String> {
        vec![]
    }
}

/// Information about a slash command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub usage: String,
    pub aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
}
