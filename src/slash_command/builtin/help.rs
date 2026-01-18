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

Session Management:
  /clear, /new           - Clear the current session and start fresh
  /undo                  - Undo the last message
  /redo                  - Redo the last message
  /compact, /summarize   - Compact the session
  /rename [name]         - Rename session
  /fork                  - Fork from message
  /timeline              - Jump to message
  /session, /resume      - List sessions

Sharing & Export:
  /share                 - Share a session
  /unshare               - Unshare a session
  /copy                  - Copy session transcript to clipboard
  /export [file]         - Export session transcript to file

Model & Agent:
  /model [name]          - Switch to a different model or open model selector
  /agent [name]          - Switch to a different agent or list available agents
  /connect [provider]    - Connect to a provider

UI & Display:
  /thinking              - Toggle thinking visibility
  /theme                 - Toggle theme
  /editor                - Open editor
  /status                - Show status
  /commands              - Show all commands

Project:
  /init                  - Create/update AGENTS.md
  /review [type] [id]    - Review changes (commit|branch|pr)

System:
  /help [command]        - Show this help or help for a specific command
  /mcp                   - Toggle MCPs
  /exit, /quit, /q       - Exit the application

Custom commands can be defined in .opencode/command/*.md files
or in your opencode.json config.

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
