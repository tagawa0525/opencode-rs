use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Undo command - undo the last message
pub struct UndoCommand;

#[async_trait]
impl SlashCommand for UndoCommand {
    fn name(&self) -> &str {
        "undo"
    }

    fn description(&self) -> &str {
        "Undo the last message"
    }

    fn usage(&self) -> &str {
        "/undo"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Undo))
    }
}
