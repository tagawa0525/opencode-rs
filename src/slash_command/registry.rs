use super::{CommandContext, CommandInfo, CommandOutput, SlashCommand};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Registry for slash commands
pub struct CommandRegistry {
    commands: RwLock<HashMap<String, Arc<dyn SlashCommand>>>,
    aliases: RwLock<HashMap<String, String>>, // alias -> command name
}

impl CommandRegistry {
    /// Create a new command registry
    pub fn new() -> Self {
        Self {
            commands: RwLock::new(HashMap::new()),
            aliases: RwLock::new(HashMap::new()),
        }
    }

    /// Register a command
    pub async fn register(&self, command: Arc<dyn SlashCommand>) {
        let name = command.name().to_string();
        let aliases: Vec<String> = command.aliases().iter().map(|s| s.to_string()).collect();

        // Register command
        self.commands.write().await.insert(name.clone(), command);

        // Register aliases
        let mut alias_map = self.aliases.write().await;
        for alias in aliases {
            alias_map.insert(alias, name.clone());
        }
    }

    /// Get a command by name or alias
    pub async fn get(&self, name: &str) -> Option<Arc<dyn SlashCommand>> {
        // First try direct lookup
        if let Some(cmd) = self.commands.read().await.get(name) {
            return Some(Arc::clone(cmd));
        }

        // Then try aliases
        if let Some(real_name) = self.aliases.read().await.get(name) {
            if let Some(cmd) = self.commands.read().await.get(real_name) {
                return Some(Arc::clone(cmd));
            }
        }

        None
    }

    /// Execute a command by name
    pub async fn execute(
        &self,
        name: &str,
        args: &str,
        ctx: &CommandContext,
    ) -> Result<CommandOutput> {
        match self.get(name).await {
            Some(cmd) => cmd.execute(args, ctx).await,
            None => bail!("Unknown command: /{}", name),
        }
    }

    /// List all registered commands
    pub async fn list(&self) -> Vec<CommandInfo> {
        let commands = self.commands.read().await;
        let mut infos: Vec<_> = commands
            .values()
            .map(|cmd| CommandInfo {
                name: cmd.name().to_string(),
                description: cmd.description().to_string(),
                usage: cmd.usage().to_string(),
                aliases: cmd.aliases().iter().map(|s| s.to_string()).collect(),
                template: None,
                agent: None,
                model: None,
                subtask: None,
            })
            .collect();

        // Sort by name
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }

    /// Get autocomplete suggestions for a partial command name
    pub async fn complete_command(&self, partial: &str) -> Vec<String> {
        let commands = self.commands.read().await;
        let aliases = self.aliases.read().await;

        let mut matches = Vec::new();

        // Check command names
        for name in commands.keys() {
            if name.starts_with(partial) {
                matches.push(format!("/{}", name));
            }
        }

        // Check aliases
        for alias in aliases.keys() {
            if alias.starts_with(partial) && !matches.contains(&format!("/{}", alias)) {
                matches.push(format!("/{}", alias));
            }
        }

        matches.sort();
        matches
    }

    /// Get autocomplete suggestions for command arguments
    pub async fn complete_args(&self, name: &str, partial: &str) -> Vec<String> {
        if let Some(cmd) = self.get(name).await {
            cmd.complete(partial).await
        } else {
            vec![]
        }
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct TestCommand {
        name: String,
        description: String,
    }

    #[async_trait]
    impl SlashCommand for TestCommand {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
            Ok(CommandOutput::text(format!("{}: {}", self.name, args)))
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = CommandRegistry::new();
        let cmd = Arc::new(TestCommand {
            name: "test".to_string(),
            description: "A test command".to_string(),
        });

        registry.register(cmd).await;

        assert!(registry.get("test").await.is_some());
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_execute() {
        let registry = CommandRegistry::new();
        let cmd = Arc::new(TestCommand {
            name: "echo".to_string(),
            description: "Echo command".to_string(),
        });

        registry.register(cmd).await;

        let ctx = CommandContext {
            session_id: "test".to_string(),
            cwd: ".".to_string(),
            root: ".".to_string(),
            extra: HashMap::new(),
        };

        let result = registry.execute("echo", "hello", &ctx).await.unwrap();
        assert_eq!(result.text, "echo: hello");
    }

    #[tokio::test]
    async fn test_list() {
        let registry = CommandRegistry::new();

        registry
            .register(Arc::new(TestCommand {
                name: "cmd1".to_string(),
                description: "First command".to_string(),
            }))
            .await;

        registry
            .register(Arc::new(TestCommand {
                name: "cmd2".to_string(),
                description: "Second command".to_string(),
            }))
            .await;

        let commands = registry.list().await;
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "cmd1");
        assert_eq!(commands[1].name, "cmd2");
    }

    #[tokio::test]
    async fn test_complete_command() {
        let registry = CommandRegistry::new();

        registry
            .register(Arc::new(TestCommand {
                name: "help".to_string(),
                description: "Help command".to_string(),
            }))
            .await;

        registry
            .register(Arc::new(TestCommand {
                name: "hello".to_string(),
                description: "Hello command".to_string(),
            }))
            .await;

        let matches = registry.complete_command("hel").await;
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&"/help".to_string()));
        assert!(matches.contains(&"/hello".to_string()));
    }
}
