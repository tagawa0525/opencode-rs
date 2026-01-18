use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Unshare command - unshare a session
pub struct UnshareCommand;

#[async_trait]
impl SlashCommand for UnshareCommand {
    fn name(&self) -> &str {
        "unshare"
    }

    fn description(&self) -> &str {
        "Unshare a session"
    }

    fn usage(&self) -> &str {
        "/unshare"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Unshare))
    }
}
