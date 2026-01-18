//! System prompt generation for LLM conversations.
//!
//! This module generates the system prompt that provides context to the LLM
//! about the environment, available tools, and instructions.

use std::path::Path;

/// Generate the environment information section of the system prompt.
///
/// This includes:
/// - Working directory
/// - Whether it's a git repo
/// - Platform
/// - Today's date
pub fn environment(cwd: &str) -> String {
    let is_git_repo = is_git_repository(cwd);
    let platform = std::env::consts::OS;
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();

    format!(
        r#"Here is some useful information about the environment you are running in:
<env>
  Working directory: {}
  Is directory a git repo: {}
  Platform: {}
  Today's date: {}
</env>
<files>
  
</files>"#,
        cwd,
        if is_git_repo { "yes" } else { "no" },
        platform,
        today
    )
}

/// Check if a directory is a git repository
fn is_git_repository(path: &str) -> bool {
    let git_dir = Path::new(path).join(".git");
    git_dir.exists()
}

/// Generate the full system prompt for a given model/provider.
///
/// Currently returns a minimal prompt with environment info.
/// TODO: Add model-specific prompts, custom instructions, etc.
pub fn generate(cwd: &str, _provider_id: &str, _model_id: &str) -> String {
    // For now, just return the environment section
    // In the future, this will include:
    // - Provider-specific base prompts
    // - Custom user instructions from AGENTS.md, CLAUDE.md, etc.
    // - Tool usage guidelines
    environment(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_generation() {
        let env = environment("/tmp/test");
        assert!(env.contains("Working directory: /tmp/test"));
        assert!(env.contains("Platform:"));
        assert!(env.contains("Today's date:"));
    }

    #[test]
    fn test_generate_includes_environment() {
        let prompt = generate("/tmp/test", "anthropic", "claude-sonnet-4-20250514");
        assert!(prompt.contains("<env>"));
        assert!(prompt.contains("</env>"));
    }
}
