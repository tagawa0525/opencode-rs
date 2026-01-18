use super::markdown::{parse_markdown_file, MarkdownFile};
use super::template::TemplateCommand;
use super::SlashCommand;
use crate::config::CommandConfig;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use walkdir::WalkDir;

/// Load slash commands from markdown files in the .opencode/command directory
pub async fn load_commands_from_directory(base_path: &Path) -> Result<Vec<Arc<dyn SlashCommand>>> {
    let mut commands = Vec::new();

    // Check for .opencode/command and .opencode/commands directories
    let possible_dirs = vec![
        base_path.join(".opencode/command"),
        base_path.join(".opencode/commands"),
    ];

    for dir in possible_dirs {
        if !dir.exists() {
            continue;
        }

        tracing::debug!("Loading commands from: {:?}", dir);

        // Walk through directory recursively
        for entry in WalkDir::new(&dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Only process .md files
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }

            match load_command_from_file(&dir, path).await {
                Ok(cmd) => {
                    tracing::debug!("Loaded command: {}", cmd.name());
                    commands.push(cmd);
                }
                Err(e) => {
                    tracing::warn!("Failed to load command from {:?}: {}", path, e);
                }
            }
        }
    }

    Ok(commands)
}

/// Load a single command from a markdown file
async fn load_command_from_file(
    base_dir: &Path,
    file_path: &Path,
) -> Result<Arc<dyn SlashCommand>> {
    // Parse the markdown file
    let markdown = parse_markdown_file(file_path)
        .await
        .with_context(|| format!("Failed to parse markdown file: {:?}", file_path))?;

    // Calculate command name from relative path
    let relative_path = file_path
        .strip_prefix(base_dir)
        .with_context(|| format!("Path {:?} is not under {:?}", file_path, base_dir))?;

    let command_name = calculate_command_name(relative_path)?;

    // Create CommandConfig from frontmatter
    let config = CommandConfig {
        template: markdown.content,
        description: markdown.frontmatter.description,
        agent: markdown.frontmatter.agent,
        model: markdown.frontmatter.model,
        subtask: markdown.frontmatter.subtask,
    };

    // Create TemplateCommand
    let cmd = TemplateCommand::new(command_name, config);

    Ok(Arc::new(cmd) as Arc<dyn SlashCommand>)
}

/// Calculate command name from relative file path
/// Examples:
/// - commit.md -> "commit"
/// - nested/child.md -> "nested/child"
fn calculate_command_name(relative_path: &Path) -> Result<String> {
    let mut parts = Vec::new();

    for component in relative_path.components() {
        if let Some(s) = component.as_os_str().to_str() {
            parts.push(s);
        }
    }

    // Join with / and remove .md extension
    let name = parts.join("/");
    let name = name.strip_suffix(".md").unwrap_or(&name);

    if name.is_empty() {
        anyhow::bail!("Command name cannot be empty");
    }

    Ok(name.to_string())
}

/// Find all .opencode directories from current path up to root
pub async fn find_opencode_directories() -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    let mut current = std::env::current_dir()?;

    loop {
        let opencode_dir = current.join(".opencode");
        if opencode_dir.exists() && opencode_dir.is_dir() {
            dirs.push(opencode_dir);
        }

        // Move to parent directory
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Also check global config directory
    if let Some(global_dir) = crate::config::Config::global_config_dir() {
        let global_opencode = global_dir.join(".opencode");
        if global_opencode.exists() && global_opencode.is_dir() {
            dirs.push(global_opencode);
        }
    }

    Ok(dirs)
}

/// Load all commands from all .opencode directories
pub async fn load_all_commands() -> Result<Vec<Arc<dyn SlashCommand>>> {
    let mut all_commands = Vec::new();
    let mut command_names = HashMap::new();

    // Find all .opencode directories
    let dirs = find_opencode_directories().await?;

    // Load commands from each directory (project configs override global)
    for dir in dirs.iter().rev() {
        let commands = load_commands_from_directory(dir).await?;

        for cmd in commands {
            let name = cmd.name().to_string();

            // Only add if not already present (project configs take precedence)
            if !command_names.contains_key(&name) {
                command_names.insert(name.clone(), ());
                all_commands.push(cmd);
            }
        }
    }

    tracing::info!("Loaded {} commands from markdown files", all_commands.len());

    Ok(all_commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_command_name() {
        assert_eq!(
            calculate_command_name(Path::new("commit.md")).unwrap(),
            "commit"
        );
        assert_eq!(
            calculate_command_name(Path::new("nested/child.md")).unwrap(),
            "nested/child"
        );
        assert_eq!(
            calculate_command_name(Path::new("a/b/c.md")).unwrap(),
            "a/b/c"
        );
    }

    #[tokio::test]
    async fn test_load_command_from_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let command_dir = temp_dir.path().join("command");
        fs::create_dir_all(&command_dir).await.unwrap();

        let file_path = command_dir.join("test.md");
        let content = r#"---
description: Test command
model: anthropic/claude-3-5-sonnet-20241022
---

This is a test command with $1 argument.
"#;
        fs::write(&file_path, content).await.unwrap();

        let cmd = load_command_from_file(&command_dir, &file_path)
            .await
            .unwrap();

        assert_eq!(cmd.name(), "test");
        assert_eq!(cmd.description(), "Test command");
    }
}
