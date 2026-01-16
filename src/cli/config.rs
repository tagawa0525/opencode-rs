//! Config management CLI commands.

use crate::config::Config;
use anyhow::Result;

/// Show current configuration
pub async fn show() -> Result<()> {
    let config = Config::load().await?;

    println!("{}", serde_json::to_string_pretty(&config)?);

    Ok(())
}

/// Show configuration file path
pub async fn path() -> Result<()> {
    if let Some(global_path) = Config::global_config_path() {
        println!("Global config: {}", global_path.display());
    }

    if let Some(global_dir) = Config::global_config_dir() {
        println!("Config directory: {}", global_dir.display());
    }

    // Check for project config
    let cwd = std::env::current_dir()?;
    let project_config = cwd.join("opencode.json");
    let project_jsonc = cwd.join("opencode.jsonc");
    let opencode_dir = cwd.join(".opencode");

    if project_config.exists() {
        println!("Project config: {}", project_config.display());
    } else if project_jsonc.exists() {
        println!("Project config: {}", project_jsonc.display());
    } else if opencode_dir.join("opencode.json").exists() {
        println!(
            "Project config: {}",
            opencode_dir.join("opencode.json").display()
        );
    } else if opencode_dir.join("opencode.jsonc").exists() {
        println!(
            "Project config: {}",
            opencode_dir.join("opencode.jsonc").display()
        );
    } else {
        println!("No project config found in {}", cwd.display());
    }

    Ok(())
}

/// Initialize configuration file with defaults
pub async fn init() -> Result<()> {
    let config_path = Config::init().await?;
    println!(
        "Created default configuration file at: {}",
        config_path.display()
    );
    println!("\nPlease edit this file to add your API keys.");
    println!("Example provider configuration:");
    println!(
        r#"
{{
  "provider": {{
    "anthropic": {{
      "key": "$ANTHROPIC_API_KEY"
    }},
    "openai": {{
      "key": "$OPENAI_API_KEY"
    }}
  }},
  "model": "anthropic/claude-3-5-sonnet-20241022"
}}
"#
    );
    Ok(())
}
