use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Commands command - show all commands
pub struct CommandsCommand;

impl Default for CommandsCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandsCommand {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SlashCommand for CommandsCommand {
    fn name(&self) -> &str {
        "commands"
    }

    fn description(&self) -> &str {
        "Show all commands"
    }

    fn usage(&self) -> &str {
        "/commands"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        // Show a message directing to /help
        let commands_list = "Type /help to see all available commands.".to_string();
        Ok(CommandOutput::text(commands_list))
    }
}
