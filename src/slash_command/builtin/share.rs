use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Share command - share a session
pub struct ShareCommand;

#[async_trait]
impl SlashCommand for ShareCommand {
    fn name(&self) -> &str {
        "share"
    }

    fn description(&self) -> &str {
        "Share a session"
    }

    fn usage(&self) -> &str {
        "/share"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Share))
    }
}
