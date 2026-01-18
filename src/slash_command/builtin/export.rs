use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Export command - export session transcript to file
pub struct ExportCommand;

#[async_trait]
impl SlashCommand for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }

    fn description(&self) -> &str {
        "Export session transcript to file"
    }

    fn usage(&self) -> &str {
        "/export [filename]"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::Export))
    }
}
