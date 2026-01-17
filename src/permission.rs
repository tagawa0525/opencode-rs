//! Permission system for tool execution.
//!
//! This module provides permission checks for tool execution, allowing
//! users to control which tools can run automatically and which require
//! confirmation.

use crate::config::{Config, PermissionAction, PermissionRule};
use anyhow::Result;
use std::collections::HashMap;

/// Permission checker for tools
pub struct PermissionChecker {
    rules: HashMap<String, PermissionAction>,
}

impl PermissionChecker {
    /// Create a new permission checker from config
    pub fn from_config(config: &Config) -> Self {
        let mut rules = HashMap::new();

        // Default permissions - most tools require confirmation
        rules.insert("read".to_string(), PermissionAction::Allow);
        rules.insert("write".to_string(), PermissionAction::Ask);
        rules.insert("edit".to_string(), PermissionAction::Ask);
        rules.insert("bash".to_string(), PermissionAction::Ask);
        rules.insert("glob".to_string(), PermissionAction::Allow);
        rules.insert("grep".to_string(), PermissionAction::Allow);
        rules.insert("doom_loop".to_string(), PermissionAction::Ask);

        // Apply config overrides
        if let Some(permissions) = &config.permission {
            for (key, rule) in permissions {
                match rule {
                    PermissionRule::Action(action) => {
                        rules.insert(key.clone(), action.clone());
                    }
                    PermissionRule::Object(obj) => {
                        // For complex objects, use the first action found
                        if let Some((_, action)) = obj.iter().next() {
                            rules.insert(key.clone(), action.clone());
                        }
                    }
                }
            }
        }

        Self { rules }
    }

    /// Check if a tool can be executed
    pub fn check_tool(&self, tool_name: &str) -> PermissionAction {
        self.rules
            .get(tool_name)
            .cloned()
            .unwrap_or(PermissionAction::Ask)
    }

    /// Check if doom loop warning should be shown
    pub fn check_doom_loop(&self) -> PermissionAction {
        self.rules
            .get("doom_loop")
            .cloned()
            .unwrap_or(PermissionAction::Ask)
    }

    /// Ask user for permission (CLI version)
    pub fn ask_user_cli(tool_name: &str, description: &str) -> Result<bool> {
        use std::io::{self, Write};

        eprintln!("\n[Permission Required]");
        eprintln!("Tool: {}", tool_name);
        eprintln!("Action: {}", description);
        eprint!("Allow execution? [y/N]: ");
        io::stderr().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let answer = input.trim().to_lowercase();
        Ok(answer == "y" || answer == "yes")
    }

    /// Check permission and ask if needed (CLI version)
    pub async fn check_and_ask_cli(&self, tool_name: &str, arguments: &str) -> Result<bool> {
        match self.check_tool(tool_name) {
            PermissionAction::Allow => Ok(true),
            PermissionAction::Deny => Ok(false),
            PermissionAction::Ask => {
                // Format arguments for display
                let args_preview = if arguments.len() > 100 {
                    format!("{}...", &arguments[..100])
                } else {
                    arguments.to_string()
                };

                let description = format!("Execute with arguments: {}", args_preview);
                Self::ask_user_cli(tool_name, &description)
            }
        }
    }

    /// Check doom loop permission and ask if needed
    pub async fn check_doom_loop_and_ask_cli(
        &self,
        tool_name: &str,
        _arguments: &str,
    ) -> Result<bool> {
        match self.check_doom_loop() {
            PermissionAction::Allow => Ok(true),
            PermissionAction::Deny => Ok(false),
            PermissionAction::Ask => {
                let description = format!(
                    "Doom loop detected: '{}' called repeatedly with same arguments",
                    tool_name
                );
                Self::ask_user_cli("doom_loop", &description)
            }
        }
    }
}

impl Default for PermissionChecker {
    fn default() -> Self {
        Self {
            rules: HashMap::from([
                ("read".to_string(), PermissionAction::Allow),
                ("write".to_string(), PermissionAction::Ask),
                ("edit".to_string(), PermissionAction::Ask),
                ("bash".to_string(), PermissionAction::Ask),
                ("glob".to_string(), PermissionAction::Allow),
                ("grep".to_string(), PermissionAction::Allow),
                ("doom_loop".to_string(), PermissionAction::Ask),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_permissions() {
        let checker = PermissionChecker::default();

        assert!(matches!(
            checker.check_tool("read"),
            PermissionAction::Allow
        ));
        assert!(matches!(checker.check_tool("write"), PermissionAction::Ask));
        assert!(matches!(checker.check_tool("bash"), PermissionAction::Ask));
        assert!(matches!(
            checker.check_tool("unknown"),
            PermissionAction::Ask
        ));
    }
}
