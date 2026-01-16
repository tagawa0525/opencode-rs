//! Run command - starts the TUI.

use anyhow::Result;

/// Execute the run command (starts TUI)
pub async fn execute(prompt: Option<String>, model: Option<String>) -> Result<()> {
    // Initialize configuration
    let config = crate::config::Config::load().await?;

    // Initialize provider registry
    crate::provider::registry().initialize(&config).await?;

    // Start TUI
    crate::tui::run(prompt, model).await
}
