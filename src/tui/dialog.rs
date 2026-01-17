//! Dialog handling for the TUI.
//!
//! This module contains dialog-related methods for the App.
//! Similar to ui/dialog.tsx in the TS version.

use super::state::App;
use super::types::{DialogState, DialogType, SelectItem};

/// Dialog-related methods for App
impl App {
    /// Open the model selector dialog
    pub fn open_model_selector(&mut self) {
        let mut items = Vec::new();

        // Only show models from available providers (with API keys)
        // By default, hide deprecated models (users can show them with a toggle)
        for provider in &self.available_providers {
            for (model_id, model) in &provider.models {
                // Skip deprecated models by default
                if matches!(model.status, crate::provider::ModelStatus::Deprecated) {
                    continue;
                }

                // Add status indicator to the label
                let status_badge = match model.status {
                    crate::provider::ModelStatus::Alpha => " [ALPHA]",
                    crate::provider::ModelStatus::Beta => " [BETA]",
                    crate::provider::ModelStatus::Active => "",
                    crate::provider::ModelStatus::Deprecated => " [DEPRECATED]",
                };

                items.push(SelectItem {
                    id: format!("{}/{}", provider.id, model_id),
                    label: format!("{}{}", model.name, status_badge),
                    description: Some(format!("{} - {}", provider.name, model_id)),
                    provider_id: Some(provider.id.clone()),
                });
            }
        }

        if items.is_empty() {
            // No available providers - open provider selector instead
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
        let mut items = Vec::new();

        match provider_id {
            "copilot" => {
                items.push(SelectItem {
                    id: "oauth".to_string(),
                    label: "Sign in with GitHub".to_string(),
                    description: Some("Use your GitHub Copilot subscription".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
                items.push(SelectItem {
                    id: "api_key".to_string(),
                    label: "Enter token manually".to_string(),
                    description: Some("Enter GITHUB_COPILOT_TOKEN directly".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
            }
            "openai" => {
                items.push(SelectItem {
                    id: "oauth".to_string(),
                    label: "Sign in with ChatGPT".to_string(),
                    description: Some("Use your ChatGPT Plus/Pro subscription".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
                items.push(SelectItem {
                    id: "api_key".to_string(),
                    label: "Enter API key".to_string(),
                    description: Some("Enter OPENAI_API_KEY directly".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
            }
            _ => {
                // For other providers, go directly to API key input
                self.open_api_key_input(provider_id);
                return;
            }
        }

        let provider_name = self
            .all_providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.to_string());

        let dialog = DialogState::new(DialogType::AuthMethodSelector, "Select Auth Method")
            .with_items(items)
            .with_message(&format!("How do you want to connect to {}?", provider_name));
        self.dialog = Some(dialog);
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
