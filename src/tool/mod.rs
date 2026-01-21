//! Tool system module.
//!
//! This module provides the tool/function calling system, similar to opencode-ts's
//! Tool module. Tools are operations that the LLM can invoke to interact with
//! the environment (read files, execute commands, etc.).

mod bash;
mod edit;
mod executor;
mod glob;
mod grep;
mod invalid;
mod question;
mod read;
mod registry;
mod todo;
mod webfetch;
mod write;

pub use bash::BashTool;
pub use edit::EditTool;
pub use executor::*;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use invalid::InvalidTool;
pub use question::QuestionTool;
pub use read::ReadTool;
pub use registry::*;
pub use todo::{TodoReadTool, TodoWriteTool};
pub use webfetch::WebFetchTool;
pub use write::WriteTool;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Permission request for tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
    pub metadata: HashMap<String, Value>,
}

/// Permission response from user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub id: String,
    pub allow: bool,
    pub scope: PermissionScope,
}

/// Permission scope - how long the permission is valid
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionScope {
    /// Allow only this single request
    #[default]
    Once,
    /// Allow for the current session (in-memory only)
    Session,
    /// Allow for this workspace/project (saved to .opencode/permissions.json)
    Workspace,
    /// Allow globally for this user (saved to ~/.opencode/permissions.json)
    Global,
}

/// Permission handler type
pub type PermissionHandler = std::sync::Arc<
    dyn Fn(PermissionRequest) -> tokio::sync::oneshot::Receiver<PermissionResponse> + Send + Sync,
>;

/// Question request for interactive prompts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRequest {
    pub id: String,
    pub questions: Vec<QuestionInfo>,
}

/// Single question information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "default_custom")]
    pub custom: bool,
}

fn default_custom() -> bool {
    true
}

/// Question option
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// Question response type - array of answer arrays (one per question)
pub type QuestionResponse = Vec<Vec<String>>;

/// Question handler type
pub type QuestionHandler = std::sync::Arc<
    dyn Fn(QuestionRequest) -> tokio::sync::oneshot::Receiver<QuestionResponse> + Send + Sync,
>;

/// Tool definition that can be sent to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name/ID
    pub name: String,
    /// Description of what the tool does
    pub description: String,
    /// JSON Schema for the tool's parameters
    pub parameters: Value,
}

/// Tool execution context
#[derive(Clone)]
pub struct ToolContext {
    /// Session ID
    pub session_id: String,
    /// Message ID
    pub message_id: String,
    /// Agent name
    pub agent: String,
    /// Abort signal
    pub abort: Option<tokio::sync::watch::Receiver<bool>>,
    /// Working directory
    pub cwd: String,
    /// Project root directory
    pub root: String,
    /// Extra context data
    pub extra: HashMap<String, Value>,
    /// Permission handler
    pub permission_handler: Option<PermissionHandler>,
    /// Question handler
    pub question_handler: Option<QuestionHandler>,
}

impl ToolContext {
    pub fn new(session_id: &str, message_id: &str, agent: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            agent: agent.to_string(),
            abort: None,
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            root: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            extra: HashMap::new(),
            permission_handler: None,
            question_handler: None,
        }
    }

    pub fn with_abort(mut self, abort: tokio::sync::watch::Receiver<bool>) -> Self {
        self.abort = Some(abort);
        self
    }

    pub fn with_cwd(mut self, cwd: String) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn with_root(mut self, root: String) -> Self {
        self.root = root;
        self
    }

    pub fn with_permission_handler(mut self, handler: PermissionHandler) -> Self {
        self.permission_handler = Some(handler);
        self
    }

    pub fn with_question_handler(mut self, handler: QuestionHandler) -> Self {
        self.question_handler = Some(handler);
        self
    }

    /// Check if execution should be aborted
    pub fn is_aborted(&self) -> bool {
        self.abort.as_ref().map(|rx| *rx.borrow()).unwrap_or(false)
    }

    /// Request permission from user
    pub async fn ask_permission(
        &self,
        permission: String,
        patterns: Vec<String>,
        always: Vec<String>,
        metadata: HashMap<String, Value>,
    ) -> Result<bool> {
        if let Some(handler) = &self.permission_handler {
            let request = PermissionRequest {
                id: uuid::Uuid::new_v4().to_string(),
                permission,
                patterns,
                always,
                metadata,
            };

            let rx = handler(request);
            match rx.await {
                Ok(response) => Ok(response.allow),
                Err(_) => anyhow::bail!("Permission request cancelled"),
            }
        } else {
            // No permission handler, default to deny for safety
            Ok(false)
        }
    }

    /// Ask user questions and get answers
    pub async fn ask_question(&self, questions: Vec<QuestionInfo>) -> Result<QuestionResponse> {
        if let Some(handler) = &self.question_handler {
            let request = QuestionRequest {
                id: uuid::Uuid::new_v4().to_string(),
                questions,
            };

            let rx = handler(request);
            match rx.await {
                Ok(response) => Ok(response),
                Err(_) => anyhow::bail!("Question request cancelled"),
            }
        } else {
            // No question handler, return empty answers
            anyhow::bail!("Question handler not available")
        }
    }
}

/// Result of tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Short title describing what happened
    pub title: String,
    /// Full output text
    pub output: String,
    /// Metadata about the execution
    pub metadata: HashMap<String, Value>,
    /// Whether the output was truncated
    #[serde(default)]
    pub truncated: bool,
    /// File attachments (for tools that produce files)
    #[serde(default)]
    pub attachments: Vec<FileAttachment>,
}

impl ToolResult {
    pub fn success(title: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            output: output.into(),
            metadata: HashMap::new(),
            truncated: false,
            attachments: Vec::new(),
        }
    }

    pub fn error(title: impl Into<String>, error: impl Into<String>) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("error".to_string(), Value::Bool(true));
        Self {
            title: title.into(),
            output: error.into(),
            metadata,
            truncated: false,
            attachments: Vec::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

/// File attachment from tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    pub path: String,
    pub mime_type: String,
    pub url: String,
}

/// Trait for implementing tools
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool's unique identifier
    fn id(&self) -> &str;

    /// Get the tool definition for the LLM
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult>;
}

/// Maximum output size before truncation (in bytes)
pub const MAX_OUTPUT_SIZE: usize = 50 * 1024; // 50KB

/// Maximum number of lines before truncation
pub const MAX_OUTPUT_LINES: usize = 2000;

/// Truncate output if it exceeds limits
pub fn truncate_output(output: &str) -> (String, bool) {
    let lines: Vec<&str> = output.lines().collect();

    // Check line count
    if lines.len() > MAX_OUTPUT_LINES {
        let truncated: String = lines[..MAX_OUTPUT_LINES].join("\n");
        let msg = format!(
            "\n\n[Output truncated: {} lines shown of {} total]",
            MAX_OUTPUT_LINES,
            lines.len()
        );
        return (truncated + &msg, true);
    }

    // Check byte size
    if output.len() > MAX_OUTPUT_SIZE {
        let mut truncated = String::new();
        let mut current_size = 0;

        for line in lines {
            if current_size + line.len() + 1 > MAX_OUTPUT_SIZE {
                break;
            }
            if !truncated.is_empty() {
                truncated.push('\n');
                current_size += 1;
            }
            truncated.push_str(line);
            current_size += line.len();
        }

        let msg = format!(
            "\n\n[Output truncated: {} bytes shown of {} total]",
            current_size,
            output.len()
        );
        return (truncated + &msg, true);
    }

    (output.to_string(), false)
}

/// Validate file path is safe (within project root)
pub fn validate_path(path: &str, root: &str) -> Result<std::path::PathBuf> {
    use std::path::Path;

    let path = Path::new(path);
    let root = Path::new(root);

    // Resolve to absolute path
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    // Canonicalize to resolve symlinks and ..
    let canonical = absolute.canonicalize().unwrap_or(absolute);
    let root_canonical = root.canonicalize().unwrap_or(root.to_path_buf());

    // Check if path is within root
    if !canonical.starts_with(&root_canonical) {
        anyhow::bail!(
            "Path '{}' is outside project root '{}'",
            canonical.display(),
            root_canonical.display()
        );
    }

    Ok(canonical)
}
