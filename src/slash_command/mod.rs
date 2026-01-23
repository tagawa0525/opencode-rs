use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod builtin;
pub mod loader;
pub mod markdown;
pub mod parser;
pub mod registry;
pub mod template;

/// Context provided to slash commands when executed
#[derive(Debug, Clone)]
pub struct CommandContext {}

/// Special actions that commands can trigger
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandAction {
    /// Open model selector dialog
    OpenModelSelector,
    /// Open agent selector dialog
    OpenAgentSelector,
    /// Open session list
    OpenSessionList,
    /// Create new session
    NewSession,
    /// Exit the application
    Exit,
    /// Open provider connection dialog
    OpenProviderConnection,
    /// Undo last message
    Undo,
    /// Redo last message
    Redo,
    /// Compact/summarize session
    Compact,
    /// Unshare session
    Unshare,
    /// Rename session (opens rename dialog)
    Rename,
    /// Copy session transcript to clipboard
    Copy,
    /// Export session transcript to file
    Export,
    /// Jump to message (timeline)
    Timeline,
    /// Fork from message
    Fork,
    /// Toggle thinking visibility
    ToggleThinking,
    /// Share session
    Share,
    /// Show status
    Status,
    /// Toggle MCPs
    ToggleMcp,
    /// Toggle theme
    ToggleTheme,
    /// Open editor
    OpenEditor,
    /// Show all commands
    ShowCommands,
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
    /// Special action to trigger
    pub action: Option<CommandAction>,
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
            action: None,
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
            action: None,
        }
    }

    /// Create output with a special action
    pub fn action(action: CommandAction) -> Self {
        Self {
            text: String::new(),
            submit_to_llm: false,
            system: None,
            agent: None,
            model: None,
            action: Some(action),
        }
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
