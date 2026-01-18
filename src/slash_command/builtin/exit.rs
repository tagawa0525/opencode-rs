use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Exit command - exits the application
pub struct ExitCommand;

#[async_trait]
impl SlashCommand for ExitCommand {
    fn name(&self) -> &str {
        "exit"
    }

    fn description(&self) -> &str {
        "Exit the application"
    }

    fn usage(&self) -> &str {
        "/exit"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["quit", "q"]
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Exit))
    }
}
