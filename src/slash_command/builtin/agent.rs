use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Agent command - switches to a different agent
pub struct AgentCommand;

#[async_trait]
impl SlashCommand for AgentCommand {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Switch to a different agent"
    }

    fn usage(&self) -> &str {
        "/agent [name]"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let args = args.trim();

        if args.is_empty() {
            Ok(CommandOutput::text(
                "Usage: /agent <name>\nExample: /agent general",
            ))
        } else {
            // This will be handled by the TUI to actually switch agents
            Ok(CommandOutput {
                text: format!("Switching to agent: {}", args),
                submit_to_llm: false,
                system: None,
                agent: Some(args.to_string()),
                model: None,
            })
        }
    }

    async fn complete(&self, _partial: &str) -> Vec<String> {
        // TODO: Integrate with config to suggest available agents
        vec![]
    }
}
