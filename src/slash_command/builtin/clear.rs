use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Clear command - clears the current session
pub struct ClearCommand;

#[async_trait]
impl SlashCommand for ClearCommand {
    fn name(&self) -> &str {
        "clear"
    }

    fn description(&self) -> &str {
        "Clear the current session"
    }

    fn usage(&self) -> &str {
        "/clear"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["new"]
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        // Trigger new session action
        Ok(CommandOutput::action(CommandAction::NewSession))
    }
}
