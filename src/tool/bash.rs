//! Bash tool for executing shell commands.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Default command timeout in milliseconds
const DEFAULT_TIMEOUT_MS: u64 = 120_000; // 2 minutes

/// Tool for executing bash commands
pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for BashTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_string(),
            description: r#"Executes a bash command in a shell session.
- Commands run in the project directory by default
- Use the workdir parameter to run in a different directory
- Output is captured and returned
- Commands time out after 2 minutes by default
- For file operations, prefer the specialized tools (Read, Write, Edit, Glob, Grep)"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory for the command"
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Timeout in milliseconds (default: 120000)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Brief description of what this command does"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("command is required"))?;

        let workdir = args
            .get("workdir")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ctx.cwd.clone());

        let timeout_ms = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_MS);

        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Execute command");

        // Check for abort
        if ctx.is_aborted() {
            return Ok(ToolResult::error(
                "Command aborted",
                "The command was aborted before execution.",
            ));
        }

        // Validate workdir exists
        let workdir_path = std::path::Path::new(&workdir);
        if !workdir_path.exists() {
            return Ok(ToolResult::error(
                format!("Directory not found: {}", workdir),
                format!("The working directory '{}' does not exist", workdir),
            ));
        }

        // Execute the command
        let start = std::time::Instant::now();

        let mut child = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Read output with timeout
        let timeout = Duration::from_millis(timeout_ms);

        let output_future = async {
            let mut stdout_reader = BufReader::new(stdout).lines();
            let mut stderr_reader = BufReader::new(stderr).lines();

            let mut stdout_lines = Vec::new();
            let mut stderr_lines = Vec::new();

            loop {
                tokio::select! {
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => stdout_lines.push(l),
                            Ok(None) => break,
                            Err(e) => {
                                stderr_lines.push(format!("Error reading stdout: {}", e));
                                break;
                            }
                        }
                    }
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(l)) => stderr_lines.push(l),
                            Ok(None) => {},
                            Err(e) => {
                                stderr_lines.push(format!("Error reading stderr: {}", e));
                            }
                        }
                    }
                }
            }

            // Drain remaining stderr
            while let Ok(Some(line)) = stderr_reader.next_line().await {
                stderr_lines.push(line);
            }

            (stdout_lines, stderr_lines)
        };

        let result = tokio::time::timeout(timeout, async {
            let (stdout_lines, stderr_lines) = output_future.await;
            let status = child.wait().await?;
            Ok::<_, anyhow::Error>((stdout_lines, stderr_lines, status))
        })
        .await;

        let duration = start.elapsed();

        match result {
            Ok(Ok((stdout_lines, stderr_lines, status))) => {
                let exit_code = status.code().unwrap_or(-1);

                // Combine output
                let mut output = stdout_lines.join("\n");
                if !stderr_lines.is_empty() {
                    if !output.is_empty() {
                        output.push_str("\n\n--- stderr ---\n");
                    }
                    output.push_str(&stderr_lines.join("\n"));
                }

                let (output, truncated) = truncate_output(&output);

                let title = if status.success() {
                    description.to_string()
                } else {
                    format!("{} (exit code {})", description, exit_code)
                };

                Ok(ToolResult {
                    title,
                    output,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("exitCode".to_string(), json!(exit_code));
                        m.insert("duration".to_string(), json!(duration.as_millis()));
                        m.insert("workdir".to_string(), json!(workdir));
                        m.insert("command".to_string(), json!(command));
                        m
                    },
                    truncated,
                    attachments: Vec::new(),
                })
            }
            Ok(Err(e)) => Ok(ToolResult::error(
                "Command failed",
                format!("Failed to execute command: {}", e),
            )),
            Err(_) => {
                // Timeout - try to kill the process
                let _ = child.kill().await;

                Ok(ToolResult::error(
                    "Command timed out",
                    format!(
                        "Command timed out after {}ms\nCommand: {}\nWorkdir: {}",
                        timeout_ms, command, workdir
                    ),
                ))
            }
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}
