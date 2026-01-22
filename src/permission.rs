//! Permission system for tool execution.
//!
//! This module provides:
//! - Doom loop permission checking for CLI mode
//! - Configuration-based permission rules

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::Result;

use crate::config::{Config, PermissionAction, PermissionRule};

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

/// Permission checker for tools (primarily used for doom loop detection in CLI mode)
pub struct PermissionChecker {
    rules: HashMap<String, PermissionAction>,
}

impl PermissionChecker {
    /// Create a new permission checker from config
    pub fn from_config(config: &Config) -> Self {
        let mut rules = Self::default_rules();

        if let Some(permissions) = &config.permission {
            for (key, rule) in permissions {
                let action = match rule {
                    PermissionRule::Action(action) => action.clone(),
                    PermissionRule::Object(obj) => obj
                        .values()
                        .next()
                        .cloned()
                        .unwrap_or(PermissionAction::Ask),
                };
                rules.insert(key.clone(), action);
            }
        }

        Self { rules }
    }

    /// Check doom loop permission and prompt user if needed
    pub async fn check_doom_loop_and_ask_cli(
        &self,
        tool_name: &str,
        _arguments: &str,
    ) -> Result<bool> {
        let action = self
            .rules
            .get("doom_loop")
            .cloned()
            .unwrap_or(PermissionAction::Ask);

        match action {
            PermissionAction::Allow => Ok(true),
            PermissionAction::Deny => Ok(false),
            PermissionAction::Ask => {
                eprintln!("\n[Permission Required]");
                eprintln!("Tool: doom_loop");
                eprintln!(
                    "Action: Doom loop detected: '{}' called repeatedly with same arguments",
                    tool_name
                );
                eprint!("Allow execution? [y/N]: ");
                io::stderr().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                let answer = input.trim().to_lowercase();
                Ok(answer == "y" || answer == "yes")
            }
        }
    }

    fn default_rules() -> HashMap<String, PermissionAction> {
        DEFAULT_PERMISSIONS
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
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
        let action = checker
            .rules
            .get("doom_loop")
            .cloned()
            .unwrap_or(PermissionAction::Deny);

        assert!(matches!(action, PermissionAction::Ask));
    }
}
