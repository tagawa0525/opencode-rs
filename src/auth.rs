//! Authentication storage module.
//!
//! This module handles persistent storage of API keys and authentication tokens.
//! Credentials are stored in ~/.local/share/opencode-rs/auth.json

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

use crate::oauth::OAuthTokenInfo;

/// Authentication storage structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthStorage {
    /// API keys by provider ID
    #[serde(default)]
    pub api_keys: HashMap<String, String>,

    /// OAuth tokens by provider ID
    #[serde(default)]
    pub oauth_tokens: HashMap<String, OAuthTokenInfo>,
}

impl AuthStorage {
    /// Get the auth storage file path
    pub fn storage_path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|p| p.join("opencode-rs").join("auth.json"))
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

    /// Set API key for a provider
    pub fn set_api_key(&mut self, provider_id: &str, api_key: &str) {
        self.api_keys
            .insert(provider_id.to_string(), api_key.to_string());
    }

    /// Set OAuth token for a provider
    pub fn set_oauth_token(&mut self, provider_id: &str, token: OAuthTokenInfo) {
        self.oauth_tokens.insert(provider_id.to_string(), token);
    }
}

/// Save an API key for a provider
pub async fn save_api_key(provider_id: &str, api_key: &str) -> Result<()> {
    let mut storage = AuthStorage::load().await.unwrap_or_default();
    storage.set_api_key(provider_id, api_key);
    storage.save().await
}

/// Save OAuth token for a provider
pub async fn save_oauth_token(provider_id: &str, token: OAuthTokenInfo) -> Result<()> {
    let mut storage = AuthStorage::load().await.unwrap_or_default();
    storage.set_oauth_token(provider_id, token);
    storage.save().await
}
