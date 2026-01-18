use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Theme command - toggle theme
pub struct ThemeCommand;

#[async_trait]
impl SlashCommand for ThemeCommand {
    fn name(&self) -> &str {
        "theme"
    }

    fn description(&self) -> &str {
        "Toggle theme"
    }

    fn usage(&self) -> &str {
        "/theme"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        Ok(CommandOutput::action(CommandAction::ToggleTheme))
    }
}
