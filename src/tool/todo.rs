//! TODO list management tools - todowrite and todoread.
//!
//! These tools allow the LLM to create and manage structured task lists
//! for tracking progress during complex operations.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;

const TODOWRITE_DESCRIPTION: &str = include_str!("../../assets/tool_descriptions/todowrite.txt");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoInfo {
    pub content: String,
    pub status: String,
    pub priority: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoWriteParams {
    pub todos: Vec<TodoInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoReadParams {}

/// Get the path to the todo file for a session
fn get_todo_path(session_id: &str) -> Result<PathBuf> {
    // Store todos in a .opencode directory in the user's home or project root
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    let opencode_dir = PathBuf::from(home).join(".opencode").join("todos");

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&opencode_dir)?;

    Ok(opencode_dir.join(format!("{}.json", session_id)))
}

/// TodoWrite tool - creates/updates TODO list
pub struct TodoWriteTool;

#[async_trait::async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> &str {
        "todowrite"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "todowrite".to_string(),
            description: TODOWRITE_DESCRIPTION.to_string(),
            parameters: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "todos": {
                        "description": "The updated todo list",
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {
                                    "type": "string",
                                    "description": "Brief description of the task"
                                },
                                "status": {
                                    "type": "string",
                                    "description": "Current status of the task: pending, in_progress, completed, cancelled"
                                },
                                "priority": {
                                    "type": "string",
                                    "description": "Priority level of the task: high, medium, low"
                                },
                                "id": {
                                    "type": "string",
                                    "description": "Unique identifier for the todo item"
                                }
                            },
                            "required": ["content", "status", "priority", "id"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let params: TodoWriteParams = serde_json::from_value(args)?;

        // Request permission
        let metadata = HashMap::new();
        ctx.ask_permission(
            "todowrite".to_string(),
            vec!["*".to_string()],
            vec!["*".to_string()],
            metadata,
        )
        .await?;

        // Save todos to file
        let todo_path = get_todo_path(&ctx.session_id)?;
        let json = serde_json::to_string_pretty(&params.todos)?;
        fs::write(&todo_path, json).await?;

        // Count non-completed todos
        let active_count = params
            .todos
            .iter()
            .filter(|t| t.status != "completed")
            .count();

        let mut metadata = HashMap::new();
        metadata.insert("todos".to_string(), serde_json::to_value(&params.todos)?);

        Ok(ToolResult {
            title: format!("{} todos", active_count),
            output: serde_json::to_string_pretty(&params.todos)?,
            metadata,
            truncated: false,
            attachments: Vec::new(),
        })
    }
}

/// TodoRead tool - reads current TODO list
pub struct TodoReadTool;

#[async_trait::async_trait]
impl Tool for TodoReadTool {
    fn id(&self) -> &str {
        "todoread"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "todoread".to_string(),
            description: "Use this tool to read your todo list".to_string(),
            parameters: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        // Request permission
        let metadata = HashMap::new();
        ctx.ask_permission(
            "todoread".to_string(),
            vec!["*".to_string()],
            vec!["*".to_string()],
            metadata,
        )
        .await?;

        // Read todos from file
        let todo_path = get_todo_path(&ctx.session_id)?;

        let todos: Vec<TodoInfo> = if todo_path.exists() {
            let content = fs::read_to_string(&todo_path).await?;
            serde_json::from_str(&content)?
        } else {
            Vec::new()
        };

        // Count non-completed todos
        let active_count = todos.iter().filter(|t| t.status != "completed").count();

        let mut metadata = HashMap::new();
        metadata.insert("todos".to_string(), serde_json::to_value(&todos)?);

        Ok(ToolResult {
            title: format!("{} todos", active_count),
            output: serde_json::to_string_pretty(&todos)?,
            metadata,
            truncated: false,
            attachments: Vec::new(),
        })
    }
}
