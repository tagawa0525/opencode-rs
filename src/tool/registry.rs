//! Tool registry for managing available tools.

use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tool registry that manages all available tools
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Create a registry with all default tools
    pub fn with_defaults() -> Self {
        let _registry = Self::new();

        // Register default tools synchronously since we're in a sync context
        let mut tools = HashMap::new();

        tools.insert(
            "read".to_string(),
            Arc::new(ReadTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "write".to_string(),
            Arc::new(WriteTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "edit".to_string(),
            Arc::new(EditTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "bash".to_string(),
            Arc::new(BashTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "glob".to_string(),
            Arc::new(GlobTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "grep".to_string(),
            Arc::new(GrepTool::new()) as Arc<dyn Tool>,
        );
        tools.insert(
            "question".to_string(),
            Arc::new(QuestionTool) as Arc<dyn Tool>,
        );
        tools.insert(
            "todowrite".to_string(),
            Arc::new(TodoWriteTool) as Arc<dyn Tool>,
        );
        tools.insert(
            "todoread".to_string(),
            Arc::new(TodoReadTool) as Arc<dyn Tool>,
        );
        tools.insert(
            "webfetch".to_string(),
            Arc::new(WebFetchTool) as Arc<dyn Tool>,
        );
        tools.insert("batch".to_string(), Arc::new(BatchTool) as Arc<dyn Tool>);

        Self {
            tools: RwLock::new(tools),
        }
    }

    /// Register a new tool
    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let mut tools = self.tools.write().await;
        tools.insert(tool.id().to_string(), tool);
    }

    /// Get a tool by ID
    pub async fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read().await;
        tools.get(id).cloned()
    }

    /// Get all registered tools
    pub async fn list(&self) -> Vec<Arc<dyn Tool>> {
        let tools = self.tools.read().await;
        tools.values().cloned().collect()
    }

    /// Get list of tool IDs
    pub fn list_tools(&self) -> Vec<String> {
        // This is a blocking method to avoid async issues in batch tool
        // We use try_read to avoid deadlocks
        if let Ok(tools) = self.tools.try_read() {
            tools.keys().cloned().collect()
        } else {
            // Fallback: return default tool list
            vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "bash".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "question".to_string(),
                "todowrite".to_string(),
                "todoread".to_string(),
                "webfetch".to_string(),
                "batch".to_string(),
            ]
        }
    }

    /// Get tool definitions for all registered tools
    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tools.values().map(|t| t.definition()).collect()
    }

    /// Get tool definitions filtered by tool IDs
    pub async fn definitions_for(&self, tool_ids: &[&str]) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        tool_ids
            .iter()
            .filter_map(|id| tools.get(*id).map(|t| t.definition()))
            .collect()
    }

    /// Execute a tool by ID
    pub async fn execute(
        &self,
        tool_id: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let tool = self
            .get(tool_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", tool_id))?;

        tool.execute(args, ctx).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// Global tool registry
static GLOBAL_REGISTRY: std::sync::LazyLock<Arc<ToolRegistry>> =
    std::sync::LazyLock::new(|| Arc::new(ToolRegistry::with_defaults()));

/// Get the global tool registry
pub fn registry() -> Arc<ToolRegistry> {
    GLOBAL_REGISTRY.clone()
}
