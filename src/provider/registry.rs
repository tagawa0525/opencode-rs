//! Provider registry for managing available AI providers.
//!
//! This module contains the `ProviderRegistry` which manages provider initialization,
//! configuration, and model loading from various sources.

use super::models_dev;
use super::types::{Model, Provider, ProviderSource};
use crate::config::Config;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Built-in provider definition
struct ProviderDef {
    id: &'static str,
    name: &'static str,
    env_vars: &'static [&'static str],
    models_dev_id: &'static str,
    fallback_ids: &'static [&'static str],
}

const BUILTIN_PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: "anthropic",
        name: "Anthropic",
        env_vars: &["ANTHROPIC_API_KEY"],
        models_dev_id: "anthropic",
        fallback_ids: &[],
    },
    ProviderDef {
        id: "openai",
        name: "OpenAI",
        env_vars: &["OPENAI_API_KEY"],
        models_dev_id: "openai",
        fallback_ids: &[],
    },
    ProviderDef {
        id: "google",
        name: "Google",
        env_vars: &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        models_dev_id: "google",
        fallback_ids: &[],
    },
    ProviderDef {
        id: "copilot",
        name: "GitHub Copilot",
        env_vars: &["GITHUB_COPILOT_TOKEN"],
        models_dev_id: "github-copilot",
        fallback_ids: &["copilot"],
    },
];

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

        self.add_builtin_providers(&mut providers).await;
        self.apply_config_overrides(&mut providers, config);
        self.load_auth_keys(&mut providers).await;
        self.load_env_keys(&mut providers);
        self.apply_provider_filters(&mut providers, config);

        Self::start_background_refresh();
        Ok(())
    }

    async fn add_builtin_providers(&self, providers: &mut HashMap<String, Provider>) {
        for def in BUILTIN_PROVIDERS {
            let models = load_models(def.models_dev_id, def.fallback_ids).await;
            providers.insert(
                def.id.to_string(),
                Provider {
                    id: def.id.to_string(),
                    name: def.name.to_string(),
                    source: ProviderSource::Custom,
                    env: def.env_vars.iter().map(|s| s.to_string()).collect(),
                    key: None,
                    options: HashMap::new(),
                    models,
                },
            );
        }
    }

    fn apply_config_overrides(&self, providers: &mut HashMap<String, Provider>, config: &Config) {
        let Some(provider_config) = &config.provider else {
            return;
        };

        for (id, cfg) in provider_config {
            let Some(provider) = providers.get_mut(id) else {
                continue;
            };

            if let Some(options) = &cfg.options {
                provider.options.extend(options.clone());
            }

            if let Some(models) = &cfg.models {
                for (model_id, model_cfg) in models {
                    if let Some(model) = provider.models.get_mut(model_id) {
                        if let Some(name) = &model_cfg.name {
                            model.name = name.clone();
                        }
                    }
                }
            }
        }
    }

    async fn load_auth_keys(&self, providers: &mut HashMap<String, Provider>) {
        let Ok(auth) = crate::auth::AuthStorage::load().await else {
            return;
        };

        for (provider_id, api_key) in &auth.api_keys {
            if let Some(p) = providers.get_mut(provider_id) {
                if p.key.is_none() {
                    p.key = Some(api_key.clone());
                    p.source = ProviderSource::Config;
                }
            }
        }

        for (provider_id, token_info) in &auth.oauth_tokens {
            if let Some(p) = providers.get_mut(provider_id) {
                if p.key.is_none() && !token_info.is_expired() {
                    p.key = Some(token_info.access.clone());
                    p.source = ProviderSource::Config;
                }
            }
        }
    }

    fn load_env_keys(&self, providers: &mut HashMap<String, Provider>) {
        for provider in providers.values_mut() {
            for env_var in &provider.env {
                if let Ok(key) = std::env::var(env_var) {
                    provider.key = Some(key);
                    provider.source = ProviderSource::Env;
                    break;
                }
            }
        }
    }

    fn apply_provider_filters(&self, providers: &mut HashMap<String, Provider>, config: &Config) {
        if let Some(disabled) = &config.disabled_providers {
            for id in disabled {
                providers.remove(id);
            }
        }

        if let Some(enabled) = &config.enabled_providers {
            let enabled_set: std::collections::HashSet<_> = enabled.iter().collect();
            providers.retain(|id, _| enabled_set.contains(id));
        }
    }

    fn start_background_refresh() {
        if models_dev::is_fetch_disabled() {
            tracing::debug!("Models fetch is disabled, skipping background refresh");
            return;
        }

        tokio::spawn(async {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
            interval.tick().await; // Skip first tick

            loop {
                interval.tick().await;
                tracing::debug!("Running scheduled models.dev refresh");
                models_dev::refresh().await;
            }
        });

        tracing::info!("Started background refresh task (every 60 minutes)");
    }

    pub async fn get(&self, id: &str) -> Option<Provider> {
        self.providers.read().await.get(id).cloned()
    }

    pub async fn get_model(&self, provider_id: &str, model_id: &str) -> Option<Model> {
        self.providers
            .read()
            .await
            .get(provider_id)
            .and_then(|p| p.models.get(model_id).cloned())
    }

    pub async fn list(&self) -> Vec<Provider> {
        self.providers.read().await.values().cloned().collect()
    }

    pub async fn list_available(&self) -> Vec<Provider> {
        self.providers
            .read()
            .await
            .values()
            .filter(|p| p.key.is_some())
            .cloned()
            .collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

async fn load_models(primary_id: &str, fallback_ids: &[&str]) -> HashMap<String, Model> {
    match models_dev::get().await {
        Ok(providers) => {
            let provider = providers
                .get(primary_id)
                .or_else(|| fallback_ids.iter().find_map(|id| providers.get(*id)));

            match provider {
                Some(p) => {
                    tracing::info!(
                        "Loaded {} {} models from models.dev",
                        p.models.len(),
                        p.name
                    );
                    p.models
                        .iter()
                        .map(|(id, m)| (id.clone(), models_dev::to_model(p, m)))
                        .collect()
                }
                None => {
                    tracing::warn!("{} provider not found in models.dev", primary_id);
                    HashMap::new()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to load models from models.dev: {}", e);
            tracing::warn!("{} models unavailable", primary_id);
            HashMap::new()
        }
    }
}

static GLOBAL_REGISTRY: std::sync::LazyLock<Arc<ProviderRegistry>> =
    std::sync::LazyLock::new(|| Arc::new(ProviderRegistry::new()));

pub fn registry() -> Arc<ProviderRegistry> {
    GLOBAL_REGISTRY.clone()
}
