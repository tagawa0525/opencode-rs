//! Provider module for AI model integration.
//!
//! This module handles integration with various AI providers (Anthropic, OpenAI, etc.)
//! and provides a unified interface for model selection and API calls.

mod models;
mod models_dev;
mod streaming;

pub use models::*;
pub use models_dev::*;
pub use streaming::*;

use crate::config::Config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Provider information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    /// Provider ID (e.g., "anthropic", "openai")
    pub id: String,
    /// Display name
    pub name: String,
    /// Source of the provider config
    pub source: ProviderSource,
    /// Environment variables for API key
    pub env: Vec<String>,
    /// API key (if directly configured)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Provider-specific options
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
    /// Available models
    pub models: HashMap<String, Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderSource {
    Env,
    Config,
    Custom,
    Api,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// Model ID
    pub id: String,
    /// Provider ID
    pub provider_id: String,
    /// Display name
    pub name: String,
    /// Model family (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// API configuration
    pub api: ModelApi,
    /// Model capabilities
    pub capabilities: ModelCapabilities,
    /// Cost per token (in dollars per million tokens)
    pub cost: ModelCost,
    /// Token limits
    pub limit: ModelLimit,
    /// Model status
    pub status: ModelStatus,
    /// Model-specific options
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
    /// Custom headers
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Release date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// Model variants
    #[serde(default)]
    pub variants: HashMap<String, HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelApi {
    /// API model ID (may differ from display ID)
    pub id: String,
    /// Base URL for API
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// NPM package for SDK (TypeScript reference)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCapabilities {
    /// Supports temperature parameter
    pub temperature: bool,
    /// Supports extended thinking/reasoning
    pub reasoning: bool,
    /// Supports file attachments
    pub attachment: bool,
    /// Supports tool/function calling
    pub toolcall: bool,
    /// Input modalities
    pub input: Modalities,
    /// Output modalities
    pub output: Modalities,
    /// Supports interleaved thinking
    #[serde(default)]
    pub interleaved: InterleavedSupport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Modalities {
    pub text: bool,
    pub audio: bool,
    pub image: bool,
    pub video: bool,
    pub pdf: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InterleavedSupport {
    Bool(bool),
    Field { field: String },
}

impl Default for InterleavedSupport {
    fn default() -> Self {
        InterleavedSupport::Bool(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCost {
    /// Cost per million input tokens
    pub input: f64,
    /// Cost per million output tokens
    pub output: f64,
    /// Cache read cost per million tokens
    #[serde(default)]
    pub cache_read: f64,
    /// Cache write cost per million tokens
    #[serde(default)]
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelLimit {
    /// Context window size
    pub context: u64,
    /// Maximum input tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    /// Maximum output tokens
    pub output: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    #[default]
    Active,
    Deprecated,
}

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

        Ok(())
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
                models: Self::anthropic_models(),
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
                models: Self::openai_models(),
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
                models: Self::google_models(),
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

    fn anthropic_models() -> HashMap<String, Model> {
        let mut models = HashMap::new();

        models.insert(
            "claude-sonnet-4-20250514".to_string(),
            Model {
                id: "claude-sonnet-4-20250514".to_string(),
                provider_id: "anthropic".to_string(),
                name: "Claude Sonnet 4".to_string(),
                family: Some("claude-4".to_string()),
                api: ModelApi {
                    id: "claude-sonnet-4-20250514".to_string(),
                    url: Some("https://api.anthropic.com".to_string()),
                    npm: Some("@ai-sdk/anthropic".to_string()),
                },
                capabilities: ModelCapabilities {
                    temperature: true,
                    reasoning: true,
                    attachment: true,
                    toolcall: true,
                    input: Modalities {
                        text: true,
                        image: true,
                        pdf: true,
                        ..Default::default()
                    },
                    output: Modalities {
                        text: true,
                        ..Default::default()
                    },
                    interleaved: InterleavedSupport::Bool(true),
                },
                cost: ModelCost {
                    input: 3.0,
                    output: 15.0,
                    cache_read: 0.3,
                    cache_write: 3.75,
                },
                limit: ModelLimit {
                    context: 200000,
                    input: None,
                    output: 16384,
                },
                status: ModelStatus::Active,
                options: HashMap::new(),
                headers: HashMap::new(),
                release_date: Some("2025-05-14".to_string()),
                variants: HashMap::new(),
            },
        );

        models.insert(
            "claude-3-5-sonnet-20241022".to_string(),
            Model {
                id: "claude-3-5-sonnet-20241022".to_string(),
                provider_id: "anthropic".to_string(),
                name: "Claude 3.5 Sonnet".to_string(),
                family: Some("claude-3.5".to_string()),
                api: ModelApi {
                    id: "claude-3-5-sonnet-20241022".to_string(),
                    url: Some("https://api.anthropic.com".to_string()),
                    npm: Some("@ai-sdk/anthropic".to_string()),
                },
                capabilities: ModelCapabilities {
                    temperature: true,
                    reasoning: false,
                    attachment: true,
                    toolcall: true,
                    input: Modalities {
                        text: true,
                        image: true,
                        pdf: true,
                        ..Default::default()
                    },
                    output: Modalities {
                        text: true,
                        ..Default::default()
                    },
                    interleaved: InterleavedSupport::Bool(false),
                },
                cost: ModelCost {
                    input: 3.0,
                    output: 15.0,
                    cache_read: 0.3,
                    cache_write: 3.75,
                },
                limit: ModelLimit {
                    context: 200000,
                    input: None,
                    output: 8192,
                },
                status: ModelStatus::Active,
                options: HashMap::new(),
                headers: HashMap::new(),
                release_date: Some("2024-10-22".to_string()),
                variants: HashMap::new(),
            },
        );

        models
    }

    fn openai_models() -> HashMap<String, Model> {
        let mut models = HashMap::new();

        models.insert(
            "gpt-4o".to_string(),
            Model {
                id: "gpt-4o".to_string(),
                provider_id: "openai".to_string(),
                name: "GPT-4o".to_string(),
                family: Some("gpt-4o".to_string()),
                api: ModelApi {
                    id: "gpt-4o".to_string(),
                    url: Some("https://api.openai.com".to_string()),
                    npm: Some("@ai-sdk/openai".to_string()),
                },
                capabilities: ModelCapabilities {
                    temperature: true,
                    reasoning: false,
                    attachment: true,
                    toolcall: true,
                    input: Modalities {
                        text: true,
                        image: true,
                        audio: true,
                        ..Default::default()
                    },
                    output: Modalities {
                        text: true,
                        audio: true,
                        ..Default::default()
                    },
                    interleaved: InterleavedSupport::Bool(false),
                },
                cost: ModelCost {
                    input: 2.5,
                    output: 10.0,
                    cache_read: 1.25,
                    cache_write: 0.0,
                },
                limit: ModelLimit {
                    context: 128000,
                    input: None,
                    output: 16384,
                },
                status: ModelStatus::Active,
                options: HashMap::new(),
                headers: HashMap::new(),
                release_date: Some("2024-05-13".to_string()),
                variants: HashMap::new(),
            },
        );

        models.insert(
            "o1".to_string(),
            Model {
                id: "o1".to_string(),
                provider_id: "openai".to_string(),
                name: "o1".to_string(),
                family: Some("o1".to_string()),
                api: ModelApi {
                    id: "o1".to_string(),
                    url: Some("https://api.openai.com".to_string()),
                    npm: Some("@ai-sdk/openai".to_string()),
                },
                capabilities: ModelCapabilities {
                    temperature: false,
                    reasoning: true,
                    attachment: true,
                    toolcall: true,
                    input: Modalities {
                        text: true,
                        image: true,
                        ..Default::default()
                    },
                    output: Modalities {
                        text: true,
                        ..Default::default()
                    },
                    interleaved: InterleavedSupport::Field {
                        field: "reasoning_content".to_string(),
                    },
                },
                cost: ModelCost {
                    input: 15.0,
                    output: 60.0,
                    cache_read: 7.5,
                    cache_write: 0.0,
                },
                limit: ModelLimit {
                    context: 200000,
                    input: None,
                    output: 100000,
                },
                status: ModelStatus::Active,
                options: HashMap::new(),
                headers: HashMap::new(),
                release_date: Some("2024-12-17".to_string()),
                variants: HashMap::new(),
            },
        );

        models
    }

    fn google_models() -> HashMap<String, Model> {
        let mut models = HashMap::new();

        models.insert(
            "gemini-2.0-flash".to_string(),
            Model {
                id: "gemini-2.0-flash".to_string(),
                provider_id: "google".to_string(),
                name: "Gemini 2.0 Flash".to_string(),
                family: Some("gemini-2.0".to_string()),
                api: ModelApi {
                    id: "gemini-2.0-flash".to_string(),
                    url: Some("https://generativelanguage.googleapis.com".to_string()),
                    npm: Some("@ai-sdk/google".to_string()),
                },
                capabilities: ModelCapabilities {
                    temperature: true,
                    reasoning: false,
                    attachment: true,
                    toolcall: true,
                    input: Modalities {
                        text: true,
                        image: true,
                        video: true,
                        audio: true,
                        pdf: true,
                    },
                    output: Modalities {
                        text: true,
                        image: true,
                        ..Default::default()
                    },
                    interleaved: InterleavedSupport::Bool(false),
                },
                cost: ModelCost {
                    input: 0.1,
                    output: 0.4,
                    cache_read: 0.025,
                    cache_write: 0.0,
                },
                limit: ModelLimit {
                    context: 1000000,
                    input: None,
                    output: 8192,
                },
                status: ModelStatus::Active,
                options: HashMap::new(),
                headers: HashMap::new(),
                release_date: Some("2024-12-11".to_string()),
                variants: HashMap::new(),
            },
        );

        models
    }

    async fn copilot_models() -> HashMap<String, Model> {
        // Try to load models dynamically from models.dev API
        // Fall back to empty HashMap if fetch is disabled or fails
        match models_dev::get().await {
            Ok(providers) => {
                // Get the github-copilot or copilot provider from models.dev
                let provider = providers.get("github-copilot")
                    .or_else(|| providers.get("copilot"));
                
                if let Some(provider) = provider {
                    tracing::info!(
                        "Loaded {} GitHub Copilot models from models.dev",
                        provider.models.len()
                    );
                    
                    // Convert models.dev models to our Model struct
                    provider
                        .models
                        .iter()
                        .map(|(id, model)| (id.clone(), models_dev::to_model(provider, model)))
                        .collect()
                } else {
                    tracing::warn!("GitHub Copilot provider not found in models.dev, using empty model list");
                    HashMap::new()
                }
            }
            Err(e) => {
                tracing::error!("Failed to load models from models.dev: {}", e);
                tracing::warn!("GitHub Copilot models unavailable - check network or set models cache");
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

// Global provider registry
lazy_static::lazy_static! {
    static ref GLOBAL_REGISTRY: Arc<ProviderRegistry> = Arc::new(ProviderRegistry::new());
}

/// Get the global provider registry
pub fn registry() -> Arc<ProviderRegistry> {
    GLOBAL_REGISTRY.clone()
}
