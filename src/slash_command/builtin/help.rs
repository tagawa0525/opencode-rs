use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Help command - shows available commands
pub struct HelpCommand;

#[async_trait]
impl SlashCommand for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show available commands"
    }

    fn usage(&self) -> &str {
        "/help [command]"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["?"]
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let help_text = if args.is_empty() {
            // Show general help
            r#"Available slash commands:

/help [command]    - Show this help or help for a specific command
/clear             - Clear the current session
/model [name]      - Switch to a different model
/agent [name]      - Switch to a different agent

You can also use custom commands defined in your opencode.json config.
Type '/' to see autocomplete suggestions for available commands.
"#
            .to_string()
        } else {
            // Show help for specific command
            format!("Help for /{}: Not yet implemented", args.trim())
        };

        Ok(CommandOutput::text(help_text))
    }
}
