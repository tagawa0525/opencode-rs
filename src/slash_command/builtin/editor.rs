use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Editor command - open editor
pub struct EditorCommand;

#[async_trait]
impl SlashCommand for EditorCommand {
    fn name(&self) -> &str {
        "editor"
    }

    fn description(&self) -> &str {
        "Open editor"
    }

    fn usage(&self) -> &str {
        "/editor"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::OpenEditor))
    }
}
