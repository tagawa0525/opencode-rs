//! Models.dev API integration for dynamic model loading
//!
//! This module fetches model definitions from https://models.dev/api.json
//! and caches them locally for offline use, similar to the TypeScript implementation.

use super::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const MODELS_DEV_API: &str = "https://models.dev/api.json";
const CACHE_FILENAME: &str = "models.json";
const CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60); // 1 hour

/// Models.dev model definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    pub release_date: String,
    #[serde(default)]
    pub attachment: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub temperature: bool,
    #[serde(default)]
    pub tool_call: bool,
    #[serde(default)]
    pub structured_output: bool,
    #[serde(default)]
    pub knowledge: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub open_weights: bool,
    #[serde(default)]
    pub interleaved: Option<InterleavedValue>,
    #[serde(default)]
    pub cost: Option<ModelsDevCost>,
    pub limit: ModelsDevLimit,
    #[serde(default)]
    pub modalities: Option<ModelsDevModalities>,
    #[serde(default)]
    pub experimental: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub provider: Option<ModelsDevProviderInfo>,
    #[serde(default)]
    pub variants: HashMap<String, HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InterleavedValue {
    Bool(bool),
    Object {
        field: String,
    },
}

impl Default for InterleavedValue {
    fn default() -> Self {
        InterleavedValue::Bool(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevCost {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
    pub context_over_200k: Option<ModelsDevCostOver200K>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevCostOver200K {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevLimit {
    pub context: u64,
    #[serde(default)]
    pub input: Option<u64>,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevModalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevProviderInfo {
    pub npm: String,
}

/// Models.dev provider definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevProvider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub doc: Option<String>,
    pub models: HashMap<String, ModelsDevModel>,
}

/// Get the cache file path
fn get_cache_path() -> Result<PathBuf> {
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?;
    let opencode_cache = cache_dir.join("opencode");
    fs::create_dir_all(&opencode_cache)?;
    Ok(opencode_cache.join(CACHE_FILENAME))
}

/// Check if the cache file exists and is fresh (less than 1 hour old)
fn is_cache_fresh() -> bool {
    let Ok(cache_path) = get_cache_path() else {
        return false;
    };
    
    let Ok(metadata) = fs::metadata(&cache_path) else {
        return false;
    };
    
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    
    elapsed < CACHE_MAX_AGE
}

/// Fetch models from models.dev API
async fn fetch_from_api() -> Result<HashMap<String, ModelsDevProvider>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    
    let response = client
        .get(MODELS_DEV_API)
        .header("User-Agent", "opencode-rs/0.1.0")
        .send()
        .await?;
    
    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch models.dev: HTTP {}", response.status());
    }
    
    let text = response.text().await?;
    let providers: HashMap<String, ModelsDevProvider> = serde_json::from_str(&text)?;
    
    Ok(providers)
}

/// Load models from cache file
fn load_from_cache() -> Result<HashMap<String, ModelsDevProvider>> {
    let cache_path = get_cache_path()?;
    let content = fs::read_to_string(cache_path)?;
    let providers: HashMap<String, ModelsDevProvider> = serde_json::from_str(&content)?;
    Ok(providers)
}

/// Save models to cache file
fn save_to_cache(providers: &HashMap<String, ModelsDevProvider>) -> Result<()> {
    let cache_path = get_cache_path()?;
    let json = serde_json::to_string_pretty(providers)?;
    fs::write(cache_path, json)?;
    Ok(())
}

/// Check if models fetch is disabled via environment variable
pub fn is_fetch_disabled() -> bool {
    std::env::var("OPENCODE_DISABLE_MODELS_FETCH")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false)
        || std::env::var("OPENCODE_DISABLE_MODELS_FETCH")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
}

/// Get models from cache or API
///
/// Strategy:
/// 1. If OPENCODE_DISABLE_MODELS_FETCH is set, only use cache
/// 2. If cache is fresh (< 1 hour), use cache
/// 3. Otherwise, try to fetch from API and update cache
/// 4. If API fetch fails, fall back to cache
pub async fn get() -> Result<HashMap<String, ModelsDevProvider>> {
    // If fetch is disabled, only use cache
    if is_fetch_disabled() {
        return load_from_cache();
    }
    
    // If cache is fresh, use it
    if is_cache_fresh() {
        if let Ok(cached) = load_from_cache() {
            return Ok(cached);
        }
    }
    
    // Try to fetch from API
    match fetch_from_api().await {
        Ok(providers) => {
            // Save to cache (ignore errors)
            let _ = save_to_cache(&providers);
            Ok(providers)
        }
        Err(e) => {
            // Fall back to cache if API fetch fails
            tracing::warn!("Failed to fetch from models.dev API: {}, using cache", e);
            load_from_cache()
        }
    }
}

/// Background refresh of models cache (fire and forget)
pub async fn refresh() {
    if is_fetch_disabled() {
        return;
    }
    
    if let Ok(providers) = fetch_from_api().await {
        let _ = save_to_cache(&providers);
        tracing::info!("Successfully refreshed models cache from models.dev");
    } else {
        tracing::debug!("Failed to refresh models cache, will retry later");
    }
}

/// Convert models.dev model to our Model struct
pub fn to_model(provider: &ModelsDevProvider, model: &ModelsDevModel) -> Model {
    let status = match model.status.as_deref() {
        Some("deprecated") => ModelStatus::Deprecated,
        Some("beta") => ModelStatus::Beta,
        Some("alpha") => ModelStatus::Alpha,
        _ if model.experimental => ModelStatus::Beta,
        _ => ModelStatus::Active,
    };
    
    let interleaved = match &model.interleaved {
        Some(InterleavedValue::Bool(b)) => InterleavedSupport::Bool(*b),
        Some(InterleavedValue::Object { field }) => InterleavedSupport::Field {
            field: field.clone(),
        },
        None => InterleavedSupport::Bool(false),
    };
    
    let modalities = model.modalities.as_ref();
    
    Model {
        id: model.id.clone(),
        provider_id: provider.id.clone(),
        name: format!("{} ({})", model.name, provider.name),
        family: model.family.clone(),
        api: ModelApi {
            id: model.id.clone(),
            url: provider.api.clone(),
            npm: model.provider.as_ref()
                .map(|p| p.npm.clone())
                .or_else(|| provider.npm.clone()),
        },
        capabilities: ModelCapabilities {
            temperature: model.temperature,
            reasoning: model.reasoning,
            attachment: model.attachment,
            toolcall: model.tool_call,
            input: Modalities {
                text: modalities.map(|m| m.input.contains(&"text".to_string())).unwrap_or(true),
                audio: modalities.map(|m| m.input.contains(&"audio".to_string())).unwrap_or(false),
                image: modalities.map(|m| m.input.contains(&"image".to_string())).unwrap_or(false),
                video: modalities.map(|m| m.input.contains(&"video".to_string())).unwrap_or(false),
                pdf: modalities.map(|m| m.input.contains(&"pdf".to_string())).unwrap_or(false),
            },
            output: Modalities {
                text: modalities.map(|m| m.output.contains(&"text".to_string())).unwrap_or(true),
                audio: modalities.map(|m| m.output.contains(&"audio".to_string())).unwrap_or(false),
                image: modalities.map(|m| m.output.contains(&"image".to_string())).unwrap_or(false),
                video: modalities.map(|m| m.output.contains(&"video".to_string())).unwrap_or(false),
                pdf: modalities.map(|m| m.output.contains(&"pdf".to_string())).unwrap_or(false),
            },
            interleaved,
        },
        cost: ModelCost {
            input: model.cost.as_ref().map(|c| c.input).unwrap_or(0.0),
            output: model.cost.as_ref().map(|c| c.output).unwrap_or(0.0),
            cache_read: model.cost.as_ref().map(|c| c.cache_read).unwrap_or(0.0),
            cache_write: model.cost.as_ref().map(|c| c.cache_write).unwrap_or(0.0),
        },
        limit: ModelLimit {
            context: model.limit.context,
            input: model.limit.input,
            output: model.limit.output,
        },
        status,
        options: model.options.clone(),
        headers: model.headers.clone(),
        release_date: Some(model.release_date.clone()),
        variants: model.variants.clone(),
    }
}
