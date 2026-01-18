use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Session command - list sessions
pub struct SessionCommand;

#[async_trait]
impl SlashCommand for SessionCommand {
    fn name(&self) -> &str {
        "session"
    }

    fn description(&self) -> &str {
        "List sessions"
    }

    fn usage(&self) -> &str {
        "/session"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["resume", "continue"]
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::OpenSessionList))
    }
}
