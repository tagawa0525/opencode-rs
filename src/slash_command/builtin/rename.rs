use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Rename command - rename the session
pub struct RenameCommand;

#[async_trait]
impl SlashCommand for RenameCommand {
    fn name(&self) -> &str {
        "rename"
    }

    fn description(&self) -> &str {
        "Rename session"
    }

    fn usage(&self) -> &str {
        "/rename [new name]"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Rename))
    }
}
