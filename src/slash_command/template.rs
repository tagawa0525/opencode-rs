use super::{CommandContext, CommandInfo, CommandOutput, SlashCommand};
use crate::config::CommandConfig;
use crate::slash_command::parser::{expand_template_async, extract_file_references};
use anyhow::Result;
use async_trait::async_trait;

/// A custom command defined via configuration with a template
pub struct TemplateCommand {
    info: CommandInfo,
}

impl TemplateCommand {
    /// Create a new template command from configuration
    pub fn new(name: String, config: CommandConfig) -> Self {
        let usage = format!("/{}", name);
        Self {
            info: CommandInfo {
                name,
                description: config.description.unwrap_or_default(),
                usage,
                aliases: vec![],
                template: Some(config.template),
                agent: config.agent,
                model: config.model,
                subtask: config.subtask,
            },
        }
    }

    /// Get the template string
    fn template(&self) -> &str {
        self.info.template.as_ref().unwrap()
    }
}

#[async_trait]
impl SlashCommand for TemplateCommand {
    fn name(&self) -> &str {
        &self.info.name
    }

    fn description(&self) -> &str {
        &self.info.description
    }

    fn usage(&self) -> &str {
        &self.info.usage
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        // Parse arguments
        let parsed_args: Vec<String> = if args.is_empty() {
            vec![]
        } else {
            super::parser::parse_quoted_args(args)
        };

        // Expand template with arguments (including shell commands)
        let expanded = expand_template_async(self.template(), &parsed_args).await?;

        // Extract file references (for future implementation)
        let file_refs = extract_file_references(&expanded);
        if !file_refs.is_empty() {
            tracing::debug!("File references found: {:?}", file_refs);
            // TODO: Handle file references - could be added to context or message
        }

        // Create output that submits to LLM
        let mut output = CommandOutput::prompt(expanded);

        // Apply agent override if specified
        if let Some(agent) = &self.info.agent {
            output = output.with_agent(agent.clone());
        }

        // Apply model override if specified
        if let Some(model) = &self.info.model {
            output = output.with_model(model.clone());
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_template_command() {
        let config = CommandConfig {
            template: "Explain $1 in detail".to_string(),
            description: Some("Explain a topic".to_string()),
            agent: None,
            model: None,
            subtask: None,
        };

        let cmd = TemplateCommand::new("explain".to_string(), config);

        let ctx = CommandContext {};

        let output = cmd.execute("Rust ownership", &ctx).await.unwrap();
        assert_eq!(output.text, "Explain Rust ownership in detail");
        assert!(output.submit_to_llm);
    }

    #[tokio::test]
    async fn test_template_command_with_overrides() {
        let config = CommandConfig {
            template: "Task: $ARGUMENTS".to_string(),
            description: Some("Run a task".to_string()),
            agent: Some("explorer".to_string()),
            model: Some("anthropic/claude-3-5-sonnet-20241022".to_string()),
            subtask: Some(true),
        };

        let cmd = TemplateCommand::new("task".to_string(), config);

        let ctx = CommandContext {};

        let output = cmd.execute("search for files", &ctx).await.unwrap();
        assert_eq!(output.text, "Task: search for files");
        assert_eq!(output.agent, Some("explorer".to_string()));
        assert_eq!(
            output.model,
            Some("anthropic/claude-3-5-sonnet-20241022".to_string())
        );
    }
}
