use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
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
        "Switch model or open model selector"
    }

    fn usage(&self) -> &str {
        "/model [provider/model]"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["models"]
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let args = args.trim();

        if args.is_empty() {
            // Open model selector dialog
            Ok(CommandOutput::action(CommandAction::OpenModelSelector))
        } else {
            // This will be handled by the TUI to actually switch models
            Ok(CommandOutput {
                text: format!("Switching to model: {}", args),
                submit_to_llm: false,
                system: None,
                agent: None,
                model: Some(args.to_string()),
                action: None,
            })
        }
    }
}
