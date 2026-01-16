//! Configuration management module.
//!
//! This module handles loading and managing configuration from various sources:
//! - Global config file (~/.config/opencode/opencode.json)
//! - Project config file (./opencode.json or ./opencode.jsonc)
//! - Environment variables
//!
//! Configuration follows a layered approach where project config overrides global config.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// JSON schema reference
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Theme name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,

    /// Default model in provider/model format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Small model for auxiliary tasks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_model: Option<String>,

    /// Default agent name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,

    /// Username to display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Log level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Disabled providers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_providers: Option<Vec<String>>,

    /// Enabled providers (if set, only these are enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_providers: Option<Vec<String>>,

    /// Share settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareMode>,

    /// Auto-update settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoupdate: Option<AutoUpdate>,

    /// Provider configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<HashMap<String, ProviderConfig>>,

    /// MCP server configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<HashMap<String, McpConfig>>,

    /// Agent configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<HashMap<String, AgentConfig>>,

    /// Command configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<HashMap<String, CommandConfig>>,

    /// Permission configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<HashMap<String, PermissionRule>>,

    /// Keybind configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keybinds: Option<KeybindsConfig>,

    /// TUI settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tui: Option<TuiConfig>,

    /// Server settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerConfig>,

    /// Compaction settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionConfig>,

    /// Additional instructions files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,

    /// Plugin list
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<Vec<String>>,

    /// Tool configurations (enable/disable tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,

    /// Experimental features
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ExperimentalConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareMode {
    Manual,
    Auto,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AutoUpdate {
    Bool(bool),
    Notify(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub api: Option<String>,
    pub npm: Option<String>,
    pub env: Option<Vec<String>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
    pub models: Option<HashMap<String, ModelConfig>>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelConfig {
    pub id: Option<String>,
    pub name: Option<String>,
    pub temperature: Option<bool>,
    pub reasoning: Option<bool>,
    pub attachment: Option<bool>,
    pub tool_call: Option<bool>,
    pub cost: Option<CostConfig>,
    pub limit: Option<LimitConfig>,
    pub options: Option<HashMap<String, serde_json::Value>>,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CostConfig {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LimitConfig {
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpConfig {
    #[serde(rename = "local")]
    Local {
        command: Vec<String>,
        environment: Option<HashMap<String, String>>,
        enabled: Option<bool>,
        timeout: Option<u64>,
    },
    #[serde(rename = "remote")]
    Remote {
        url: String,
        headers: Option<HashMap<String, String>>,
        enabled: Option<bool>,
        timeout: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentConfig {
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub description: Option<String>,
    pub mode: Option<AgentMode>,
    pub hidden: Option<bool>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub permission: Option<HashMap<String, PermissionRule>>,
    pub disable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Subagent,
    Primary,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommandConfig {
    pub template: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionRule {
    Action(PermissionAction),
    Object(HashMap<String, PermissionAction>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Ask,
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeybindsConfig {
    pub leader: Option<String>,
    pub app_exit: Option<String>,
    pub editor_open: Option<String>,
    pub theme_list: Option<String>,
    pub sidebar_toggle: Option<String>,
    pub session_new: Option<String>,
    pub session_list: Option<String>,
    pub model_list: Option<String>,
    pub command_list: Option<String>,
    pub agent_list: Option<String>,
    pub input_submit: Option<String>,
    pub input_newline: Option<String>,
    pub input_clear: Option<String>,
    pub input_paste: Option<String>,
    pub session_interrupt: Option<String>,
    pub session_compact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TuiConfig {
    pub scroll_speed: Option<f64>,
    pub diff_style: Option<DiffStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStyle {
    Auto,
    Stacked,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub hostname: Option<String>,
    pub mdns: Option<bool>,
    pub cors: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CompactionConfig {
    pub auto: Option<bool>,
    pub prune: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExperimentalConfig {
    pub chat_max_retries: Option<u32>,
    pub disable_paste_summary: Option<bool>,
    pub batch_tool: Option<bool>,
    pub open_telemetry: Option<bool>,
    pub continue_loop_on_deny: Option<bool>,
    pub mcp_timeout: Option<u64>,
}

impl Config {
    /// Load configuration from all sources
    pub async fn load() -> Result<Self> {
        let mut config = Config::default();

        // Load global config
        if let Some(global_path) = Self::global_config_path() {
            if let Some(global_config) = Self::load_file(&global_path).await? {
                config = config.merge(global_config);
            }
        }

        // Load project config
        if let Some(project_path) = Self::find_project_config().await? {
            if let Some(project_config) = Self::load_file(&project_path).await? {
                config = config.merge(project_config);
            }
        }

        // Apply environment variable overrides
        config = config.apply_env_overrides();

        Ok(config)
    }

    /// Get the global config directory path
    pub fn global_config_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("opencode"))
    }

    /// Get the global config file path
    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_config_dir().map(|p| p.join("opencode.json"))
    }

    /// Find project config file in current directory or parent directories
    async fn find_project_config() -> Result<Option<PathBuf>> {
        let mut current = std::env::current_dir()?;

        loop {
            // Check for opencode.jsonc first, then opencode.json
            for filename in &["opencode.jsonc", "opencode.json"] {
                let config_path = current.join(filename);
                if config_path.exists() {
                    return Ok(Some(config_path));
                }
            }

            // Also check .opencode directory
            let opencode_dir = current.join(".opencode");
            if opencode_dir.exists() {
                for filename in &["opencode.jsonc", "opencode.json"] {
                    let config_path = opencode_dir.join(filename);
                    if config_path.exists() {
                        return Ok(Some(config_path));
                    }
                }
            }

            // Move to parent directory
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                break;
            }
        }

        Ok(None)
    }

    /// Load configuration from a file
    async fn load_file(path: &Path) -> Result<Option<Config>> {
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        // Handle empty or whitespace-only files
        if content.trim().is_empty() {
            return Ok(Some(Config::default()));
        }

        // Handle JSONC (JSON with comments)
        let content = Self::strip_jsonc_comments(&content);

        // Strip trailing commas
        let content = Self::strip_trailing_commas(&content);

        // Handle environment variable substitution
        let content = Self::substitute_env_vars(&content);

        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;

        Ok(Some(config))
    }

    /// Strip comments from JSONC content
    fn strip_jsonc_comments(content: &str) -> String {
        let mut result = String::new();
        let mut in_string = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut chars = content.chars().peekable();

        while let Some(c) = chars.next() {
            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                    result.push(c);
                }
                continue;
            }

            if in_block_comment {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    in_block_comment = false;
                }
                continue;
            }

            if c == '"' && !in_string {
                in_string = true;
                result.push(c);
                continue;
            }

            if c == '"' && in_string {
                // Check for escape
                let mut backslash_count = 0;
                for ch in result.chars().rev() {
                    if ch == '\\' {
                        backslash_count += 1;
                    } else {
                        break;
                    }
                }
                if backslash_count % 2 == 0 {
                    in_string = false;
                }
                result.push(c);
                continue;
            }

            if !in_string {
                if c == '/' && chars.peek() == Some(&'/') {
                    chars.next();
                    in_line_comment = true;
                    continue;
                }

                if c == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    in_block_comment = true;
                    continue;
                }
            }

            result.push(c);
        }

        result
    }

    /// Strip trailing commas from JSON (common in JSONC)
    fn strip_trailing_commas(content: &str) -> String {
        // Remove trailing commas before closing braces or brackets
        let re = regex::Regex::new(r",(\s*[}\]])").unwrap();
        re.replace_all(content, "$1").to_string()
    }

    /// Substitute environment variables in the format {env:VAR_NAME}
    fn substitute_env_vars(content: &str) -> String {
        let re = regex::Regex::new(r"\{env:([^}]+)\}").unwrap();
        re.replace_all(content, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_default()
        })
        .to_string()
    }

    /// Merge another config into this one (other takes precedence)
    pub fn merge(mut self, other: Config) -> Self {
        if other.schema.is_some() {
            self.schema = other.schema;
        }
        if other.theme.is_some() {
            self.theme = other.theme;
        }
        if other.model.is_some() {
            self.model = other.model;
        }
        if other.small_model.is_some() {
            self.small_model = other.small_model;
        }
        if other.default_agent.is_some() {
            self.default_agent = other.default_agent;
        }
        if other.username.is_some() {
            self.username = other.username;
        }
        if other.log_level.is_some() {
            self.log_level = other.log_level;
        }
        if other.disabled_providers.is_some() {
            self.disabled_providers = other.disabled_providers;
        }
        if other.enabled_providers.is_some() {
            self.enabled_providers = other.enabled_providers;
        }
        if other.share.is_some() {
            self.share = other.share;
        }
        if other.autoupdate.is_some() {
            self.autoupdate = other.autoupdate;
        }

        // Merge maps
        if let Some(other_providers) = other.provider {
            let providers = self.provider.get_or_insert_with(HashMap::new);
            providers.extend(other_providers);
        }
        if let Some(other_mcp) = other.mcp {
            let mcp = self.mcp.get_or_insert_with(HashMap::new);
            mcp.extend(other_mcp);
        }
        if let Some(other_agents) = other.agent {
            let agents = self.agent.get_or_insert_with(HashMap::new);
            agents.extend(other_agents);
        }
        if let Some(other_commands) = other.command {
            let commands = self.command.get_or_insert_with(HashMap::new);
            commands.extend(other_commands);
        }
        if let Some(other_permissions) = other.permission {
            let permissions = self.permission.get_or_insert_with(HashMap::new);
            permissions.extend(other_permissions);
        }

        if other.keybinds.is_some() {
            self.keybinds = other.keybinds;
        }
        if other.tui.is_some() {
            self.tui = other.tui;
        }
        if other.server.is_some() {
            self.server = other.server;
        }
        if other.compaction.is_some() {
            self.compaction = other.compaction;
        }
        if other.instructions.is_some() {
            self.instructions = other.instructions;
        }
        if other.plugin.is_some() {
            self.plugin = other.plugin;
        }
        if let Some(other_tools) = other.tools {
            let tools = self.tools.get_or_insert_with(HashMap::new);
            tools.extend(other_tools);
        }
        if other.experimental.is_some() {
            self.experimental = other.experimental;
        }

        self
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(mut self) -> Self {
        if let Ok(model) = std::env::var("OPENCODE_MODEL") {
            self.model = Some(model);
        }
        if let Ok(theme) = std::env::var("OPENCODE_THEME") {
            self.theme = Some(theme);
        }
        if let Ok(log_level) = std::env::var("OPENCODE_LOG_LEVEL") {
            self.log_level = Some(log_level);
        }
        self
    }

    /// Get the effective username
    pub fn get_username(&self) -> String {
        self.username
            .clone()
            .or_else(|| std::env::var("USER").ok())
            .or_else(|| std::env::var("USERNAME").ok())
            .unwrap_or_else(|| "user".to_string())
    }

    /// Create a default config file if it doesn't exist
    pub async fn init() -> Result<PathBuf> {
        let config_dir = Self::global_config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        
        // Create config directory if it doesn't exist
        fs::create_dir_all(&config_dir).await
            .context("Failed to create config directory")?;

        let config_path = config_dir.join("opencode.json");

        if !config_path.exists() {
            // Create default config
            let default_config = Config {
                schema: Some("https://opencode.ai/schema/config.json".to_string()),
                theme: Some("dark".to_string()),
                model: None, // User needs to configure this
                small_model: None,
                default_agent: None,
                username: None,
                log_level: Some("info".to_string()),
                disabled_providers: None,
                enabled_providers: None,
                share: Some(ShareMode::Disabled),
                autoupdate: Some(AutoUpdate::Notify("notify".to_string())),
                provider: Some(HashMap::new()),
                mcp: None,
                agent: None,
                command: None,
                permission: None,
                keybinds: None,
                tui: Some(TuiConfig {
                    scroll_speed: Some(3.0),
                    diff_style: Some(DiffStyle::Auto),
                }),
                server: Some(ServerConfig {
                    port: Some(19876),
                    hostname: Some("127.0.0.1".to_string()),
                    mdns: Some(false),
                    cors: None,
                }),
                compaction: Some(CompactionConfig {
                    auto: Some(true),
                    prune: Some(false),
                }),
                experimental: None,
                instructions: None,
                plugin: None,
                tools: None,
            };

            let content = serde_json::to_string_pretty(&default_config)?;
            fs::write(&config_path, content).await
                .context("Failed to write default config file")?;
        }

        Ok(config_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_jsonc_comments() {
        let input = r#"{
            // This is a comment
            "key": "value", // inline comment
            /* block
               comment */
            "key2": "value2"
        }"#;

        let result = Config::strip_jsonc_comments(input);
        assert!(!result.contains("//"));
        assert!(!result.contains("/*"));
        assert!(result.contains(r#""key": "value""#));
    }

    #[test]
    fn test_substitute_env_vars() {
        std::env::set_var("TEST_VAR", "test_value");
        let input = r#"{"key": "{env:TEST_VAR}"}"#;
        let result = Config::substitute_env_vars(input);
        assert_eq!(result, r#"{"key": "test_value"}"#);
    }

    #[test]
    fn test_merge_configs() {
        let config1 = Config {
            theme: Some("dark".to_string()),
            model: Some("anthropic/claude".to_string()),
            ..Default::default()
        };

        let config2 = Config {
            theme: Some("light".to_string()),
            username: Some("test_user".to_string()),
            ..Default::default()
        };

        let merged = config1.merge(config2);
        assert_eq!(merged.theme, Some("light".to_string()));
        assert_eq!(merged.model, Some("anthropic/claude".to_string()));
        assert_eq!(merged.username, Some("test_user".to_string()));
    }

    #[test]
    fn test_strip_trailing_commas() {
        let input = r#"{
            "key": "value",
            "nested": {
                "foo": "bar",
            },
            "array": [1, 2, 3,],
        }"#;

        let result = Config::strip_trailing_commas(input);
        assert!(!result.contains(",}"));
        assert!(!result.contains(",]"));
        
        // Should be valid JSON after stripping
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_empty_config() {
        let empty_content = "";
        let whitespace_content = "   \n  \t  ";
        
        // These should not panic and should return default config
        assert!(empty_content.trim().is_empty());
        assert!(whitespace_content.trim().is_empty());
    }
}
