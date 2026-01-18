use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Connect command - opens provider connection dialog
pub struct ConnectCommand;

#[async_trait]
impl SlashCommand for ConnectCommand {
    fn name(&self) -> &str {
        "connect"
    }

    fn description(&self) -> &str {
        "Connect to a provider"
    }

    fn usage(&self) -> &str {
        "/connect [provider]"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::OpenProviderConnection))
    }
}
