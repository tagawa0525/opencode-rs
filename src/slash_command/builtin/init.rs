use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Init command - create/update AGENTS.md
pub struct InitCommand;

#[async_trait]
impl SlashCommand for InitCommand {
    fn name(&self) -> &str {
        "init"
    }

    fn description(&self) -> &str {
        "Create/update AGENTS.md"
    }

    fn usage(&self) -> &str {
        "/init"
    }

    async fn execute(&self, _args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let prompt = r#"Please create or update the AGENTS.md file in the current directory.

This file should define custom agents for this project. Each agent should have:
- A clear name and purpose
- Specific instructions tailored to this project
- Tools and capabilities they can use

Please analyze the current project structure and create appropriate agents."#;

        Ok(CommandOutput::prompt(prompt))
    }
}
