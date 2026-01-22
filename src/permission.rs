//! Permission system for tool execution.
//!
//! This module provides permission checks for tool execution, allowing
//! users to control which tools can run automatically and which require
//! confirmation.

use crate::config::{Config, PermissionAction, PermissionRule};
use anyhow::Result;
use std::collections::HashMap;

/// Default permission rules for tools
const DEFAULT_PERMISSIONS: &[(&str, PermissionAction)] = &[
    ("read", PermissionAction::Allow),
    ("write", PermissionAction::Ask),
    ("edit", PermissionAction::Ask),
    ("bash", PermissionAction::Ask),
    ("glob", PermissionAction::Allow),
    ("grep", PermissionAction::Allow),
    ("question", PermissionAction::Allow),
    ("todowrite", PermissionAction::Allow),
    ("todoread", PermissionAction::Allow),
    ("webfetch", PermissionAction::Ask),
    ("doom_loop", PermissionAction::Ask),
];

/// Permission checker for tools
pub struct PermissionChecker {
    rules: HashMap<String, PermissionAction>,
}

impl PermissionChecker {
    /// Create rules from default permissions
    fn default_rules() -> HashMap<String, PermissionAction> {
        DEFAULT_PERMISSIONS
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// Create a new permission checker from config
    pub fn from_config(config: &Config) -> Self {
        let mut rules = Self::default_rules();

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
            rules: Self::default_rules(),
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
