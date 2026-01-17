use super::{CommandContext, CommandInfo, CommandOutput, SlashCommand};
use crate::config::CommandConfig;
use crate::slash_command::parser::expand_template;
use anyhow::Result;
use async_trait::async_trait;

/// A custom command defined via configuration with a template
pub struct TemplateCommand {
    info: CommandInfo,
}

impl TemplateCommand {
    /// Create a new template command from configuration
    pub fn new(name: String, config: CommandConfig) -> Self {
        Self {
            info: CommandInfo {
                name,
                description: config.description.unwrap_or_default(),
                usage: String::new(), // Will be generated from template
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
        &self.info.name
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        // Parse arguments
        let parsed_args: Vec<String> = if args.is_empty() {
            vec![]
        } else {
            super::parser::parse_quoted_args(args)
        };

        // Expand template with arguments
        let expanded = expand_template(self.template(), &parsed_args);

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

        let ctx = CommandContext {
            session_id: "test".to_string(),
            cwd: ".".to_string(),
            root: ".".to_string(),
            extra: Default::default(),
        };

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

        let ctx = CommandContext {
            session_id: "test".to_string(),
            cwd: ".".to_string(),
            root: ".".to_string(),
            extra: Default::default(),
        };

        let output = cmd.execute("search for files", &ctx).await.unwrap();
        assert_eq!(output.text, "Task: search for files");
        assert_eq!(output.agent, Some("explorer".to_string()));
        assert_eq!(
            output.model,
            Some("anthropic/claude-3-5-sonnet-20241022".to_string())
        );
    }
}
