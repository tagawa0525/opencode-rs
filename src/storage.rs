//! JSON file-based storage system.
//!
//! This module provides persistent storage using JSON files, similar to
//! opencode-ts's Storage module. Data is stored in a hierarchical directory
//! structure based on keys.

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Base directory for storage
    pub base_path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let base = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("opencode-rs");
        Self { base_path: base }
    }
}

/// Main storage struct
pub struct Storage {
    config: StorageConfig,
    /// Simple in-memory cache for frequently accessed data
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl Storage {
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create storage with default configuration
    pub fn with_defaults() -> Self {
        Self::new(StorageConfig::default())
    }

    /// Convert a key path to a file path
    fn key_to_path(&self, key: &[&str]) -> PathBuf {
        let mut path = self.config.base_path.clone();
        for part in key {
            path = path.join(part);
        }
        path.with_extension("json")
    }

    /// Convert a key path to a cache key string
    fn key_to_string(key: &[&str]) -> String {
        key.join("/")
    }

    /// Write data to storage
    pub async fn write<T: Serialize>(&self, key: &[&str], data: &T) -> Result<()> {
        let path = self.key_to_path(key);
        let cache_key = Self::key_to_string(key);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        // Serialize data
        let json =
            serde_json::to_string_pretty(data).with_context(|| "Failed to serialize data")?;

        // Write to file
        fs::write(&path, &json)
            .await
            .with_context(|| format!("Failed to write to {:?}", path))?;

        // Update cache
        let mut cache = self.cache.write().await;
        cache.insert(cache_key, json);

        Ok(())
    }

    /// Read data from storage
    pub async fn read<T: DeserializeOwned>(&self, key: &[&str]) -> Result<Option<T>> {
        let path = self.key_to_path(key);
        let cache_key = Self::key_to_string(key);

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(json) = cache.get(&cache_key) {
                let data: T = serde_json::from_str(json)
                    .with_context(|| "Failed to deserialize cached data")?;
                return Ok(Some(data));
            }
        }

        // Read from file
        if !path.exists() {
            return Ok(None);
        }

        let json = fs::read_to_string(&path)
            .await
            .with_context(|| format!("Failed to read from {:?}", path))?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, json.clone());
        }

        let data: T = serde_json::from_str(&json)
            .with_context(|| format!("Failed to deserialize data from {:?}", path))?;

        Ok(Some(data))
    }

    /// Update data in storage using a closure
    pub async fn update<T, F>(&self, key: &[&str], updater: F) -> Result<T>
    where
        T: Serialize + DeserializeOwned + Default,
        F: FnOnce(&mut T),
    {
        let mut data: T = self.read(key).await?.unwrap_or_default();
        updater(&mut data);
        self.write(key, &data).await?;
        Ok(data)
    }

    /// Remove data from storage
    pub async fn remove(&self, key: &[&str]) -> Result<()> {
        let path = self.key_to_path(key);
        let cache_key = Self::key_to_string(key);

        // Remove from cache
        {
            let mut cache = self.cache.write().await;
            cache.remove(&cache_key);
        }

        // Remove file if it exists
        if path.exists() {
            fs::remove_file(&path)
                .await
                .with_context(|| format!("Failed to remove {:?}", path))?;
        }

        Ok(())
    }

    /// List all items in a directory
    pub async fn list(&self, key: &[&str]) -> Result<Vec<Vec<String>>> {
        let mut path = self.config.base_path.clone();
        for part in key {
            path = path.join(part);
        }

        let mut items = Vec::new();

        if !path.exists() {
            return Ok(items);
        }

        let mut entries = fs::read_dir(&path)
            .await
            .with_context(|| format!("Failed to read directory {:?}", path))?;

        while let Some(entry) = entries.next_entry().await? {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Remove .json extension if present
            let name = if let Some(stripped) = name.strip_suffix(".json") {
                stripped.to_string()
            } else {
                name.to_string()
            };

            let mut item_key: Vec<String> = key.iter().map(|s| s.to_string()).collect();
            item_key.push(name);
            items.push(item_key);
        }

        // Sort by key (which includes ID, so this sorts chronologically)
        items.sort();

        Ok(items)
    }

    /// Check if a key exists
    pub async fn exists(&self, key: &[&str]) -> bool {
        let path = self.key_to_path(key);
        path.exists()
    }

    /// Get the base storage path
    pub fn base_path(&self) -> &Path {
        &self.config.base_path
    }
}

// Global storage instance
static GLOBAL_STORAGE: std::sync::LazyLock<Storage> =
    std::sync::LazyLock::new(Storage::with_defaults);

/// Get the global storage instance
pub fn global() -> &'static Storage {
    &GLOBAL_STORAGE
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_write_read() {
        let dir = tempdir().unwrap();
        let storage = Storage::new(StorageConfig {
            base_path: dir.path().to_path_buf(),
        });

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.write(&["test", "item1"], &data).await.unwrap();

        let read: Option<TestData> = storage.read(&["test", "item1"]).await.unwrap();
        assert_eq!(read, Some(data));
    }

    #[tokio::test]
    async fn test_update() {
        let dir = tempdir().unwrap();
        let storage = Storage::new(StorageConfig {
            base_path: dir.path().to_path_buf(),
        });

        let initial = TestData {
            name: "initial".to_string(),
            value: 1,
        };

        storage.write(&["test", "item"], &initial).await.unwrap();

        let updated: TestData = storage
            .update(&["test", "item"], |data: &mut TestData| {
                data.value = 100;
            })
            .await
            .unwrap();

        assert_eq!(updated.value, 100);
        assert_eq!(updated.name, "initial");
    }

    #[tokio::test]
    async fn test_list() {
        let dir = tempdir().unwrap();
        let storage = Storage::new(StorageConfig {
            base_path: dir.path().to_path_buf(),
        });

        let data = TestData {
            name: "test".to_string(),
            value: 1,
        };

        storage.write(&["items", "a"], &data).await.unwrap();
        storage.write(&["items", "b"], &data).await.unwrap();
        storage.write(&["items", "c"], &data).await.unwrap();

        let items = storage.list(&["items"]).await.unwrap();
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn test_remove() {
        let dir = tempdir().unwrap();
        let storage = Storage::new(StorageConfig {
            base_path: dir.path().to_path_buf(),
        });

        let data = TestData {
            name: "test".to_string(),
            value: 1,
        };

        storage.write(&["test", "item"], &data).await.unwrap();
        assert!(storage.exists(&["test", "item"]).await);

        storage.remove(&["test", "item"]).await.unwrap();
        assert!(!storage.exists(&["test", "item"]).await);
    }
}
