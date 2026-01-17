use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Model command - switches to a different model
pub struct ModelCommand;

#[async_trait]
impl SlashCommand for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Switch to a different model"
    }

    fn usage(&self) -> &str {
        "/model [provider/model]"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let args = args.trim();

        if args.is_empty() {
            Ok(CommandOutput::text(
                "Usage: /model <provider/model>\nExample: /model anthropic/claude-3-5-sonnet-20241022"
            ))
        } else {
            // This will be handled by the TUI to actually switch models
            Ok(CommandOutput {
                text: format!("Switching to model: {}", args),
                submit_to_llm: false,
                system: None,
                agent: None,
                model: Some(args.to_string()),
            })
        }
    }

    async fn complete(&self, _partial: &str) -> Vec<String> {
        // TODO: Integrate with provider registry to suggest available models
        vec![]
    }
}
