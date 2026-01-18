use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Redo command - redo the last message
pub struct RedoCommand;

#[async_trait]
impl SlashCommand for RedoCommand {
    fn name(&self) -> &str {
        "redo"
    }

    fn description(&self) -> &str {
        "Redo the last message"
    }

    fn usage(&self) -> &str {
        "/redo"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Redo))
    }
}
