//! Provider registry for managing available AI providers.
//!
//! This module contains the `ProviderRegistry` which manages provider initialization,
//! configuration, and model loading from various sources.

use super::models_dev;
use super::types::{Model, ModelStatus, Provider, ProviderSource};
use crate::config::Config;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Provider registry for managing available providers
pub struct ProviderRegistry {
    providers: RwLock<HashMap<String, Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Initialize the registry with built-in and configured providers
    pub async fn initialize(&self, config: &Config) -> Result<()> {
        let mut providers = self.providers.write().await;

        // Add built-in providers
        self.add_builtin_providers(&mut providers).await?;

        // Apply config overrides
        if let Some(provider_config) = &config.provider {
            for (id, cfg) in provider_config {
                if let Some(provider) = providers.get_mut(id) {
                    // Apply config options
                    if let Some(options) = &cfg.options {
                        provider.options.extend(options.clone());
                    }
                    // Apply model overrides
                    if let Some(models) = &cfg.models {
                        for (model_id, model_cfg) in models {
                            if let Some(model) = provider.models.get_mut(model_id) {
                                if let Some(name) = &model_cfg.name {
                                    model.name = name.clone();
                                }
                                // Apply other overrides...
                            }
                        }
                    }
                }
            }
        }

        // Load saved API keys from auth storage
        if let Ok(auth) = crate::auth::AuthStorage::load().await {
            // Load API keys
            for (provider_id, api_key) in &auth.api_keys {
                if let Some(provider) = providers.get_mut(provider_id) {
                    if provider.key.is_none() {
                        provider.key = Some(api_key.clone());
                        provider.source = ProviderSource::Config;
                    }
                }
            }

            // Load OAuth tokens
            for (provider_id, token_info) in &auth.oauth_tokens {
                if let Some(provider) = providers.get_mut(provider_id) {
                    if provider.key.is_none() && !token_info.is_expired() {
                        provider.key = Some(token_info.access.clone());
                        provider.source = ProviderSource::Config;
                    }
                }
            }
        }

        // Check for API keys in environment (overrides saved keys)
        for provider in providers.values_mut() {
            for env_var in &provider.env {
                if let Ok(key) = std::env::var(env_var) {
                    provider.key = Some(key);
                    provider.source = ProviderSource::Env;
                    break;
                }
            }
        }

        // Filter disabled providers
        if let Some(disabled) = &config.disabled_providers {
            for id in disabled {
                providers.remove(id);
            }
        }

        // Apply enabled_providers filter if set
        if let Some(enabled) = &config.enabled_providers {
            let enabled_set: std::collections::HashSet<_> = enabled.iter().collect();
            providers.retain(|id, _| enabled_set.contains(id));
        }

        // Start background refresh task (every 60 minutes)
        Self::start_background_refresh();

        Ok(())
    }

    /// Start a background task to refresh models cache every 60 minutes
    fn start_background_refresh() {
        if models_dev::is_fetch_disabled() {
            tracing::debug!("Models fetch is disabled, skipping background refresh");
            return;
        }

        tokio::spawn(async {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
            // Skip the first tick (immediate fire)
            interval.tick().await;

            loop {
                interval.tick().await;
                tracing::debug!("Running scheduled models.dev refresh");
                models_dev::refresh().await;
            }
        });

        tracing::info!("Started background refresh task (every 60 minutes)");
    }

    /// Add built-in provider definitions
    async fn add_builtin_providers(&self, providers: &mut HashMap<String, Provider>) -> Result<()> {
        // Anthropic
        providers.insert(
            "anthropic".to_string(),
            Provider {
                id: "anthropic".to_string(),
                name: "Anthropic".to_string(),
                source: ProviderSource::Custom,
                env: vec!["ANTHROPIC_API_KEY".to_string()],
                key: None,
                options: HashMap::new(),
                models: Self::anthropic_models().await,
            },
        );

        // OpenAI
        providers.insert(
            "openai".to_string(),
            Provider {
                id: "openai".to_string(),
                name: "OpenAI".to_string(),
                source: ProviderSource::Custom,
                env: vec!["OPENAI_API_KEY".to_string()],
                key: None,
                options: HashMap::new(),
                models: Self::openai_models().await,
            },
        );

        // Google
        providers.insert(
            "google".to_string(),
            Provider {
                id: "google".to_string(),
                name: "Google".to_string(),
                source: ProviderSource::Custom,
                env: vec!["GOOGLE_API_KEY".to_string(), "GEMINI_API_KEY".to_string()],
                key: None,
                options: HashMap::new(),
                models: Self::google_models().await,
            },
        );

        // GitHub Copilot
        providers.insert(
            "copilot".to_string(),
            Provider {
                id: "copilot".to_string(),
                name: "GitHub Copilot".to_string(),
                source: ProviderSource::Custom,
                env: vec!["GITHUB_COPILOT_TOKEN".to_string()],
                key: None,
                options: HashMap::new(),
                models: Self::copilot_models().await,
            },
        );

        Ok(())
    }

    async fn anthropic_models() -> HashMap<String, Model> {
        Self::load_models_from_dev("anthropic", &[]).await
    }

    async fn openai_models() -> HashMap<String, Model> {
        Self::load_models_from_dev("openai", &[]).await
    }

    async fn google_models() -> HashMap<String, Model> {
        Self::load_models_from_dev("google", &[]).await
    }

    async fn copilot_models() -> HashMap<String, Model> {
        Self::load_models_from_dev("github-copilot", &["copilot"]).await
    }

    /// Load models from models.dev API for a specific provider
    ///
    /// # Arguments
    /// * `primary_id` - Primary provider ID to look for in models.dev
    /// * `fallback_ids` - Alternative provider IDs to try if primary not found
    async fn load_models_from_dev(
        primary_id: &str,
        fallback_ids: &[&str],
    ) -> HashMap<String, Model> {
        // Try to load models dynamically from models.dev API
        // Fall back to empty HashMap if fetch is disabled or fails
        match models_dev::get().await {
            Ok(providers) => {
                // Try primary ID first, then fallbacks
                let provider = providers
                    .get(primary_id)
                    .or_else(|| fallback_ids.iter().find_map(|id| providers.get(*id)));

                if let Some(provider) = provider {
                    tracing::info!(
                        "Loaded {} {} models from models.dev",
                        provider.models.len(),
                        provider.name
                    );

                    // Convert models.dev models to our Model struct
                    provider
                        .models
                        .iter()
                        .map(|(id, model)| (id.clone(), models_dev::to_model(provider, model)))
                        .collect()
                } else {
                    tracing::warn!(
                        "{} provider not found in models.dev, using empty model list",
                        primary_id
                    );
                    HashMap::new()
                }
            }
            Err(e) => {
                tracing::error!("Failed to load models from models.dev: {}", e);
                tracing::warn!(
                    "{} models unavailable - check network or set models cache",
                    primary_id
                );
                HashMap::new()
            }
        }
    }

    /// Get a provider by ID
    pub async fn get(&self, id: &str) -> Option<Provider> {
        let providers = self.providers.read().await;
        providers.get(id).cloned()
    }

    /// Get a model by provider and model ID
    pub async fn get_model(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        let providers = self.providers.read().await;
        providers
            .get(provider_id)
            .and_then(|p| p.models.get(model_id).cloned())
    }

    /// List all providers
    pub async fn list(&self) -> Vec<Provider> {
        let providers = self.providers.read().await;
        providers.values().cloned().collect()
    }

    /// List all available providers (with API keys)
    pub async fn list_available(&self) -> Vec<Provider> {
        let providers = self.providers.read().await;
        providers
            .values()
            .filter(|p| p.key.is_some())
            .cloned()
            .collect()
    }

    /// List all models across all providers
    ///
    /// # Arguments
    /// * `include_deprecated` - If false, filters out deprecated models
    pub async fn list_all_models(&self, include_deprecated: bool) -> Vec<(String, Model)> {
        let providers = self.providers.read().await;
        let mut models = Vec::new();

        for provider in providers.values() {
            for (model_id, model) in &provider.models {
                // Skip deprecated models if requested
                if !include_deprecated && matches!(model.status, ModelStatus::Deprecated) {
                    continue;
                }

                models.push((format!("{}/{}", provider.id, model_id), model.clone()));
            }
        }

        // Sort models by status (Active first, then Beta, Alpha, Deprecated)
        models.sort_by(|(_, a), (_, b)| {
            let a_priority = match a.status {
                ModelStatus::Active => 0,
                ModelStatus::Beta => 1,
                ModelStatus::Alpha => 2,
                ModelStatus::Deprecated => 3,
            };
            let b_priority = match b.status {
                ModelStatus::Active => 0,
                ModelStatus::Beta => 1,
                ModelStatus::Alpha => 2,
                ModelStatus::Deprecated => 3,
            };

            a_priority
                .cmp(&b_priority)
                .then_with(|| a.name.cmp(&b.name))
        });

        models
    }

    /// Get the default model
    pub async fn default_model(&self, config: &Config) -> Option<(String, String)> {
        // Check config for default model
        if let Some(model) = &config.model {
            let parts: Vec<&str> = model.splitn(2, '/').collect();
            if parts.len() == 2 {
                return Some((parts[0].to_string(), parts[1].to_string()));
            }
        }

        // Find first available provider with a model
        let providers = self.providers.read().await;
        for provider in providers.values() {
            if provider.key.is_some() {
                if let Some(model_id) = provider.models.keys().next() {
                    return Some((provider.id.clone(), model_id.clone()));
                }
            }
        }

        None
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Global provider registry
static GLOBAL_REGISTRY: std::sync::LazyLock<Arc<ProviderRegistry>> =
    std::sync::LazyLock::new(|| Arc::new(ProviderRegistry::new()));

/// Get the global provider registry
pub fn registry() -> Arc<ProviderRegistry> {
    GLOBAL_REGISTRY.clone()
}
