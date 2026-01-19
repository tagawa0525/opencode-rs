//! Dialog handling for the TUI.
//!
//! This module contains dialog-related methods for the App and
//! the dialog input handler function.
//! Similar to ui/dialog.tsx in the TS version.

use anyhow::Result;
use crossterm::event::KeyCode;
use tokio::sync::mpsc;

use super::oauth_flow::{start_copilot_oauth_flow, start_openai_oauth_flow};
use super::state::App;
use super::types::{AppEvent, DialogState, DialogType, SelectItem};
use crate::config::Config;
use crate::provider;

/// Dialog-related methods for App
impl App {
    /// Open the model selector dialog
    pub fn open_model_selector(&mut self) {
        let items = self.collect_available_models();

        if items.is_empty() {
            self.open_provider_selector();
            return;
        }

        let dialog = DialogState::new(
            DialogType::ModelSelector,
            "Select Model (deprecated models hidden)",
        )
        .with_items(items);
        self.dialog = Some(dialog);
    }

    /// Collect available models from providers, excluding deprecated ones
    fn collect_available_models(&self) -> Vec<SelectItem> {
        self.available_providers
            .iter()
            .flat_map(|provider| {
                provider
                    .models
                    .iter()
                    .filter(|(_, model)| {
                        !matches!(model.status, crate::provider::ModelStatus::Deprecated)
                    })
                    .map(move |(model_id, model)| SelectItem {
                        id: format!("{}/{}", provider.id, model_id),
                        label: format!("{}{}", model.name, model_status_badge(model.status)),
                        description: Some(format!("{} - {}", provider.name, model_id)),
                        provider_id: Some(provider.id.clone()),
                    })
            })
            .collect()
    }

    /// Open the provider selector dialog
    pub fn open_provider_selector(&mut self) {
        let items: Vec<SelectItem> = self
            .all_providers
            .iter()
            .map(|p| {
                let has_key = p.key.is_some();
                SelectItem {
                    id: p.id.clone(),
                    label: p.name.clone(),
                    description: Some(if has_key {
                        "Connected".to_string()
                    } else {
                        format!("Set {}", p.env.first().unwrap_or(&"API_KEY".to_string()))
                    }),
                    provider_id: None,
                }
            })
            .collect();

        let dialog = DialogState::new(DialogType::ProviderSelector, "Connect Provider")
            .with_items(items)
            .with_message("Select a provider to configure");
        self.dialog = Some(dialog);
    }

    /// Open provider connection dialog (alias for open_provider_selector)
    pub fn open_provider_connection(&mut self) {
        self.open_provider_selector();
    }

    /// Open API key input dialog for a provider
    pub fn open_api_key_input(&mut self, provider_id: &str) {
        let provider = self.all_providers.iter().find(|p| p.id == provider_id);
        let env_var = provider
            .and_then(|p| p.env.first())
            .cloned()
            .unwrap_or_else(|| "API_KEY".to_string());

        let mut dialog = DialogState::new(DialogType::ApiKeyInput, "Enter API Key");
        dialog.message = Some(format!("Enter API key for {} ({})", provider_id, env_var));
        dialog.input_value = String::new();
        // Store provider_id in the first item
        dialog.items = vec![SelectItem {
            id: provider_id.to_string(),
            label: env_var,
            description: None,
            provider_id: Some(provider_id.to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Open auth method selector for a provider
    pub fn open_auth_method_selector(&mut self, provider_id: &str) {
        let items = match get_auth_method_items(provider_id) {
            Some(items) => items,
            None => {
                // For other providers, go directly to API key input
                self.open_api_key_input(provider_id);
                return;
            }
        };

        let provider_name = self.get_provider_name(provider_id);
        let dialog = DialogState::new(DialogType::AuthMethodSelector, "Select Auth Method")
            .with_items(items)
            .with_message(&format!("How do you want to connect to {}?", provider_name));
        self.dialog = Some(dialog);
    }

    /// Get provider name by ID
    fn get_provider_name(&self, provider_id: &str) -> String {
        self.all_providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.to_string())
    }

    /// Start GitHub Copilot OAuth device flow
    pub fn start_copilot_oauth(&mut self) {
        let mut dialog = DialogState::new(DialogType::OAuthWaiting, "GitHub Copilot Sign In");
        dialog.message = Some("Requesting device code...".to_string());
        dialog.items = vec![SelectItem {
            id: "copilot".to_string(),
            label: "copilot".to_string(),
            description: None,
            provider_id: Some("copilot".to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Update dialog with device code info
    pub fn show_device_code(&mut self, user_code: &str, verification_uri: &str, device_code: &str) {
        if let Some(dialog) = &mut self.dialog {
            dialog.dialog_type = DialogType::OAuthDeviceCode;
            dialog.user_code = Some(user_code.to_string());
            dialog.verification_uri = Some(verification_uri.to_string());
            dialog.device_code = Some(device_code.to_string());
            dialog.message = Some(format!(
                "Go to: {}\n\nEnter code: {}",
                verification_uri, user_code
            ));
        }
    }

    /// Start OpenAI OAuth PKCE flow
    pub fn start_openai_oauth(&mut self) {
        let mut dialog = DialogState::new(DialogType::OAuthWaiting, "ChatGPT Sign In");
        dialog.message = Some("Opening browser for authentication...".to_string());
        dialog.items = vec![SelectItem {
            id: "openai".to_string(),
            label: "openai".to_string(),
            description: None,
            provider_id: Some("openai".to_string()),
        }];
        self.dialog = Some(dialog);
    }
}

// ============================================================================
// Dialog Input Handlers
// ============================================================================

/// Handle input for model/provider selector dialogs
async fn handle_selector_input(app: &mut App, key_code: KeyCode) -> Result<()> {
    match key_code {
        KeyCode::Esc => {
            // Close dialog, but if model not configured, quit
            if !app.model_configured
                && app.dialog.as_ref().map(|d| &d.dialog_type) == Some(&DialogType::ModelSelector)
            {
                app.should_quit = true;
            }
            app.close_dialog();
        }
        KeyCode::Enter => {
            handle_selector_enter(app).await?;
        }
        KeyCode::Up => {
            if let Some(dialog) = &mut app.dialog {
                dialog.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(dialog) = &mut app.dialog {
                dialog.move_down();
            }
        }
        KeyCode::Char(c) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.search_query.push(c);
                dialog.update_filter();
            }
        }
        KeyCode::Backspace => {
            if let Some(dialog) = &mut app.dialog {
                dialog.search_query.pop();
                dialog.update_filter();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle Enter key in selector dialogs
async fn handle_selector_enter(app: &mut App) -> Result<()> {
    let (item_id, dialog_type) = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return Ok(()),
        };
        let item = match dialog.selected_item() {
            Some(i) => i,
            None => return Ok(()),
        };
        (item.id.clone(), dialog.dialog_type.clone())
    };

    match dialog_type {
        DialogType::ModelSelector => {
            if let Some((provider_id, model_id)) = provider::parse_model_string(&item_id) {
                app.set_model(&provider_id, &model_id).await?;
            }
        }
        DialogType::ProviderSelector => {
            let has_key = app
                .all_providers
                .iter()
                .find(|p| p.id == item_id)
                .map(|p| p.key.is_some())
                .unwrap_or(false);

            if has_key {
                app.close_dialog();
                app.open_model_selector();
            } else {
                app.open_auth_method_selector(&item_id);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle input for API key input dialog
async fn handle_api_key_input(app: &mut App, key_code: KeyCode) -> Result<()> {
    match key_code {
        KeyCode::Esc => {
            app.open_provider_selector();
        }
        KeyCode::Enter => {
            handle_api_key_submit(app).await?;
        }
        KeyCode::Char(c) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.input_value.push(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(dialog) = &mut app.dialog {
                dialog.input_value.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle API key submission
async fn handle_api_key_submit(app: &mut App) -> Result<()> {
    let (api_key, provider_id, env_var) = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return Ok(()),
        };
        let first_item = match dialog.items.first() {
            Some(i) => i,
            None => return Ok(()),
        };
        (
            dialog.input_value.clone(),
            first_item.id.clone(),
            first_item.label.clone(),
        )
    };

    if api_key.is_empty() {
        return Ok(());
    }

    // Set environment variable for current session
    std::env::set_var(&env_var, &api_key);

    // Re-initialize registry
    let config = Config::load().await?;
    provider::registry().initialize(&config).await?;

    // Update cached providers
    app.all_providers = provider::registry().list().await;
    app.available_providers = provider::registry().list_available().await;

    // Close dialog and open model selector
    app.close_dialog();
    app.open_model_selector();

    // Save to auth file
    if let Err(e) = crate::auth::save_api_key(&provider_id, &api_key).await {
        eprintln!("Warning: Failed to save API key: {}", e);
    }

    Ok(())
}

/// Handle input for auth method selector dialog
async fn handle_auth_method_input(
    app: &mut App,
    key_code: KeyCode,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    match key_code {
        KeyCode::Esc => {
            app.open_provider_selector();
        }
        KeyCode::Enter => {
            handle_auth_method_enter(app, event_tx).await;
        }
        KeyCode::Up => {
            if let Some(dialog) = &mut app.dialog {
                dialog.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(dialog) = &mut app.dialog {
                dialog.move_down();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle Enter key in auth method selector
async fn handle_auth_method_enter(app: &mut App, event_tx: &mpsc::Sender<AppEvent>) {
    let (auth_method, provider_id) = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return,
        };
        let item = match dialog.selected_item() {
            Some(i) => i,
            None => return,
        };
        (
            item.id.clone(),
            item.provider_id.clone().unwrap_or_default(),
        )
    };

    match auth_method.as_str() {
        "oauth" => {
            start_oauth_flow(app, &provider_id, event_tx);
        }
        "api_key" => {
            app.open_api_key_input(&provider_id);
        }
        _ => {}
    }
}

/// Start OAuth flow for a provider
fn start_oauth_flow(app: &mut App, provider_id: &str, event_tx: &mpsc::Sender<AppEvent>) {
    match provider_id {
        "copilot" => {
            app.start_copilot_oauth();
            let tx = event_tx.clone();
            tokio::spawn(async move {
                start_copilot_oauth_flow(tx).await;
            });
        }
        "openai" => {
            app.start_openai_oauth();
            let tx = event_tx.clone();
            tokio::spawn(async move {
                start_openai_oauth_flow(tx).await;
            });
        }
        _ => {}
    }
}

/// Handle input for permission request dialog
async fn handle_permission_input(
    app: &mut App,
    key_code: KeyCode,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let permission_id = app
        .dialog
        .as_ref()
        .and_then(|d| d.permission_request.as_ref())
        .map(|r| r.id.clone());

    let id = match permission_id {
        Some(id) => id,
        None => return Ok(()),
    };

    // Map key to permission response (allow, scope)
    let response = match key_code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some((true, crate::tool::PermissionScope::Once)),
        KeyCode::Char('s') | KeyCode::Char('S') => {
            Some((true, crate::tool::PermissionScope::Session))
        }
        KeyCode::Char('w') | KeyCode::Char('W') => {
            Some((true, crate::tool::PermissionScope::Workspace))
        }
        KeyCode::Char('g') | KeyCode::Char('G') => {
            Some((true, crate::tool::PermissionScope::Global))
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Some((false, crate::tool::PermissionScope::Once))
        }
        _ => None,
    };

    if let Some((allow, scope)) = response {
        app.close_dialog();
        let _ = event_tx
            .send(AppEvent::PermissionResponse { id, allow, scope })
            .await;
    }
    Ok(())
}

/// Handle input when a dialog is open
pub async fn handle_dialog_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    let dialog_type = app.dialog.as_ref().map(|d| d.dialog_type.clone());

    match dialog_type {
        Some(DialogType::ModelSelector) | Some(DialogType::ProviderSelector) => {
            handle_selector_input(app, key.code).await?;
        }
        Some(DialogType::ApiKeyInput) => {
            handle_api_key_input(app, key.code).await?;
        }
        Some(DialogType::AuthMethodSelector) => {
            handle_auth_method_input(app, key.code, &event_tx).await?;
        }
        Some(DialogType::OAuthDeviceCode) | Some(DialogType::OAuthWaiting) => {
            if key.code == KeyCode::Esc {
                app.open_provider_selector();
            }
        }
        Some(DialogType::PermissionRequest) => {
            handle_permission_input(app, key.code, &event_tx).await?;
        }
        _ => {
            if key.code == KeyCode::Esc {
                app.close_dialog();
            }
        }
    }

    Ok(())
}

/// Get status badge text for a model status
fn model_status_badge(status: crate::provider::ModelStatus) -> &'static str {
    match status {
        crate::provider::ModelStatus::Alpha => " [ALPHA]",
        crate::provider::ModelStatus::Beta => " [BETA]",
        crate::provider::ModelStatus::Active => "",
        crate::provider::ModelStatus::Deprecated => " [DEPRECATED]",
    }
}

/// Get auth method items for a provider, if OAuth is supported
fn get_auth_method_items(provider_id: &str) -> Option<Vec<SelectItem>> {
    let (oauth_label, oauth_desc, key_label, key_desc) = match provider_id {
        "copilot" => (
            "Sign in with GitHub",
            "Use your GitHub Copilot subscription",
            "Enter token manually",
            "Enter GITHUB_COPILOT_TOKEN directly",
        ),
        "openai" => (
            "Sign in with ChatGPT",
            "Use your ChatGPT Plus/Pro subscription",
            "Enter API key",
            "Enter OPENAI_API_KEY directly",
        ),
        _ => return None,
    };

    Some(vec![
        SelectItem {
            id: "oauth".to_string(),
            label: oauth_label.to_string(),
            description: Some(oauth_desc.to_string()),
            provider_id: Some(provider_id.to_string()),
        },
        SelectItem {
            id: "api_key".to_string(),
            label: key_label.to_string(),
            description: Some(key_desc.to_string()),
            provider_id: Some(provider_id.to_string()),
        },
    ])
}
