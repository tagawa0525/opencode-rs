use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Compact command - compact/summarize the session
pub struct CompactCommand;

#[async_trait]
impl SlashCommand for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self) -> &str {
        "Compact the session"
    }

    fn usage(&self) -> &str {
        "/compact"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["summarize"]
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let prompt = r#"Please summarize our conversation so far in a concise way, preserving the key information and context. Focus on:
- Main topics discussed
- Important decisions made
- Key technical details
- Current state and next steps

After the summary, we'll continue our conversation with this condensed context."#;

        Ok(CommandOutput::prompt(prompt))
    }
}
