use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Thinking command - toggle thinking visibility
pub struct ThinkingCommand;

#[async_trait]
impl SlashCommand for ThinkingCommand {
    fn name(&self) -> &str {
        "thinking"
    }

    fn description(&self) -> &str {
        "Toggle thinking visibility"
    }

    fn usage(&self) -> &str {
        "/thinking"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::ToggleThinking))
    }
}
