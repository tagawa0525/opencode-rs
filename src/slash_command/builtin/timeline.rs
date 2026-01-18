use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Timeline command - jump to a specific message
pub struct TimelineCommand;

#[async_trait]
impl SlashCommand for TimelineCommand {
    fn name(&self) -> &str {
        "timeline"
    }

    fn description(&self) -> &str {
        "Jump to message"
    }

    fn usage(&self) -> &str {
        "/timeline"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Timeline))
    }
}
