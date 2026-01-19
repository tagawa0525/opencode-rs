//! Provider and Model type definitions.
//!
//! This module contains the core type definitions for AI providers and models,
//! including capabilities, costs, and configuration options.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    #[default]
    Active,
    Deprecated,
}
