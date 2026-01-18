use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Fork command - fork from a specific message
pub struct ForkCommand;

#[async_trait]
impl SlashCommand for ForkCommand {
    fn name(&self) -> &str {
        "fork"
    }

    fn description(&self) -> &str {
        "Fork from message"
    }

    fn usage(&self) -> &str {
        "/fork"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Fork))
    }
}
