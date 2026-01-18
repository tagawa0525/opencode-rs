use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Copy command - copy session transcript to clipboard
pub struct CopyCommand;

#[async_trait]
impl SlashCommand for CopyCommand {
    fn name(&self) -> &str {
        "copy"
    }

    fn description(&self) -> &str {
        "Copy session transcript to clipboard"
    }

    fn usage(&self) -> &str {
        "/copy"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Copy))
    }
}
