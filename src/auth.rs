//! Authentication storage module.
//!
//! This module handles persistent storage of API keys and authentication tokens.
//! Credentials are stored in ~/.local/share/opencode/auth.json

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

/// Authentication storage structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthStorage {
    /// API keys by provider ID
    #[serde(default)]
    pub api_keys: HashMap<String, String>,

    /// OAuth tokens by provider ID (for future use)
    #[serde(default)]
    pub tokens: HashMap<String, OAuthToken>,
}

/// OAuth token structure (for future use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl AuthStorage {
    /// Get the auth storage file path
    pub fn storage_path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|p| p.join("opencode").join("auth.json"))
    }

    /// Load auth storage from disk
    pub async fn load() -> Result<Self> {
        let path = Self::storage_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine auth storage path"))?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .await
            .with_context(|| format!("Failed to read auth file: {:?}", path))?;

        let storage: AuthStorage = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse auth file: {:?}", path))?;

        Ok(storage)
    }

    /// Save auth storage to disk
    pub async fn save(&self) -> Result<()> {
        let path = Self::storage_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine auth storage path"))?;

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create auth directory: {:?}", parent))?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)
            .await
            .with_context(|| format!("Failed to write auth file: {:?}", path))?;

        Ok(())
    }

    /// Get API key for a provider
    pub fn get_api_key(&self, provider_id: &str) -> Option<&String> {
        self.api_keys.get(provider_id)
    }

    /// Set API key for a provider
    pub fn set_api_key(&mut self, provider_id: &str, api_key: &str) {
        self.api_keys
            .insert(provider_id.to_string(), api_key.to_string());
    }

    /// Remove API key for a provider
    pub fn remove_api_key(&mut self, provider_id: &str) {
        self.api_keys.remove(provider_id);
    }
}

/// Save an API key for a provider
pub async fn save_api_key(provider_id: &str, api_key: &str) -> Result<()> {
    let mut storage = AuthStorage::load().await.unwrap_or_default();
    storage.set_api_key(provider_id, api_key);
    storage.save().await
}

/// Load an API key for a provider
pub async fn load_api_key(provider_id: &str) -> Option<String> {
    AuthStorage::load()
        .await
        .ok()
        .and_then(|s| s.get_api_key(provider_id).cloned())
}

/// Remove an API key for a provider
pub async fn remove_api_key(provider_id: &str) -> Result<()> {
    let mut storage = AuthStorage::load().await.unwrap_or_default();
    storage.remove_api_key(provider_id);
    storage.save().await
}

/// Load all saved API keys into environment variables
pub async fn load_saved_keys_to_env() -> Result<()> {
    let storage = AuthStorage::load().await?;

    // Map provider IDs to environment variable names
    let env_map: HashMap<&str, &str> = [
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("google", "GOOGLE_API_KEY"),
        ("gemini", "GEMINI_API_KEY"),
    ]
    .into_iter()
    .collect();

    for (provider_id, api_key) in &storage.api_keys {
        if let Some(&env_var) = env_map.get(provider_id.as_str()) {
            // Only set if not already set
            if std::env::var(env_var).is_err() {
                std::env::set_var(env_var, api_key);
            }
        }
    }

    Ok(())
}
