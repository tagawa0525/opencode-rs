use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Mcp command - toggle MCPs
pub struct McpCommand;

#[async_trait]
impl SlashCommand for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Toggle MCPs"
    }

    fn usage(&self) -> &str {
        "/mcp"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::ToggleMcp))
    }
}
