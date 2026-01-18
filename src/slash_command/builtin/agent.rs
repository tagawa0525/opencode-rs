use crate::slash_command::{CommandAction, CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Agent command - switches to a different agent or opens agent selector
pub struct AgentCommand;

#[async_trait]
impl SlashCommand for AgentCommand {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Switch to a different agent or list available agents"
    }

    fn usage(&self) -> &str {
        "/agent [name]"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["agents"]
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let args = args.trim();

        if args.is_empty() {
            // Open agent selector dialog
            Ok(CommandOutput::action(CommandAction::OpenAgentSelector))
        } else {
            // Switch to specific agent
            Ok(CommandOutput {
                text: format!("Switching to agent: {}", args),
                submit_to_llm: false,
                system: None,
                agent: Some(args.to_string()),
                model: None,
                action: None,
            })
        }
    }

    async fn complete(&self, _partial: &str) -> Vec<String> {
        // TODO: Integrate with config to suggest available agents
        vec![]
    }
}
