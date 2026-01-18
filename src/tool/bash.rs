//! Bash tool for executing shell commands.

use super::*;
use anyhow::Result;
use serde_json::{json, Value};
use std::process::{ExitStatus, Stdio};
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

/// Arguments parsed from the tool input
struct BashArgs {
    command: String,
    workdir: String,
    timeout_ms: u64,
    description: String,
}

/// Output from command execution
struct CommandOutput {
    stdout_lines: Vec<String>,
    stderr_lines: Vec<String>,
    status: ExitStatus,
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
        let args = parse_args(args, ctx)?;

        // Check for abort
        if ctx.is_aborted() {
            return Ok(ToolResult::error(
                "Command aborted",
                "The command was aborted before execution.",
            ));
        }

        // Validate workdir exists
        if let Some(err) = validate_workdir(&args.workdir) {
            return Ok(err);
        }

        // Request permission before executing bash command
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("command".to_string(), serde_json::json!(args.command));
        metadata.insert("workdir".to_string(), serde_json::json!(args.workdir));
        metadata.insert("timeout".to_string(), serde_json::json!(args.timeout_ms));

        let allowed = ctx
            .ask_permission(
                "bash".to_string(),
                vec![args.command.clone()],
                vec!["*".to_string()],
                metadata,
            )
            .await?;

        if !allowed {
            return Ok(ToolResult::error(
                "Permission Denied",
                format!(
                    "User denied permission to execute bash command: {}",
                    args.command
                ),
            ));
        }

        // Execute the command
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(args.timeout_ms);

        match run_command(&args.command, &args.workdir, timeout).await {
            Ok(output) => {
                let duration = start.elapsed();
                Ok(build_success_result(&args, output, duration))
            }
            Err(CommandError::Timeout) => Ok(ToolResult::error(
                "Command timed out",
                format!(
                    "Command timed out after {}ms\nCommand: {}\nWorkdir: {}",
                    args.timeout_ms, args.command, args.workdir
                ),
            )),
            Err(CommandError::Execution(e)) => Ok(ToolResult::error(
                "Command failed",
                format!("Failed to execute command: {}", e),
            )),
        }
    }
}

/// Parse arguments from the tool input
fn parse_args(args: Value, ctx: &ToolContext) -> Result<BashArgs> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("command is required"))?
        .to_string();

    // Resolve workdir: if not absolute, join with cwd (like TypeScript version)
    let workdir_arg = args
        .get("workdir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| ctx.cwd.clone());

    let workdir = if std::path::Path::new(&workdir_arg).is_absolute() {
        workdir_arg
    } else {
        std::path::Path::new(&ctx.cwd)
            .join(&workdir_arg)
            .to_string_lossy()
            .to_string()
    };

    let timeout_ms = args
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_MS);

    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("Execute command")
        .to_string();

    Ok(BashArgs {
        command,
        workdir,
        timeout_ms,
        description,
    })
}

/// Validate that the working directory exists
fn validate_workdir(workdir: &str) -> Option<ToolResult> {
    let workdir_path = std::path::Path::new(workdir);
    if !workdir_path.exists() {
        Some(ToolResult::error(
            format!("Directory not found: {}", workdir),
            format!("The working directory '{}' does not exist", workdir),
        ))
    } else {
        None
    }
}

/// Error types for command execution
enum CommandError {
    Timeout,
    Execution(anyhow::Error),
}

/// Run the bash command with timeout
async fn run_command(
    command: &str,
    workdir: &str,
    timeout: Duration,
) -> std::result::Result<CommandOutput, CommandError> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CommandError::Execution(e.into()))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let result = tokio::time::timeout(timeout, async {
        let (stdout_lines, stderr_lines) = read_output(stdout, stderr).await;
        let status = child.wait().await?;
        Ok::<_, anyhow::Error>(CommandOutput {
            stdout_lines,
            stderr_lines,
            status,
        })
    })
    .await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(CommandError::Execution(e)),
        Err(_) => {
            // Timeout - try to kill the process
            let _ = child.kill().await;
            Err(CommandError::Timeout)
        }
    }
}

/// Read stdout and stderr from the process
async fn read_output(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
) -> (Vec<String>, Vec<String>) {
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
}

/// Build the success result from command output
fn build_success_result(
    args: &BashArgs,
    output: CommandOutput,
    duration: std::time::Duration,
) -> ToolResult {
    let exit_code = output.status.code().unwrap_or(-1);

    // Combine output
    let mut combined_output = output.stdout_lines.join("\n");
    if !output.stderr_lines.is_empty() {
        if !combined_output.is_empty() {
            combined_output.push_str("\n\n--- stderr ---\n");
        }
        combined_output.push_str(&output.stderr_lines.join("\n"));
    }

    let (output_text, truncated) = truncate_output(&combined_output);

    let title = if output.status.success() {
        args.description.clone()
    } else {
        format!("{} (exit code {})", args.description, exit_code)
    };

    ToolResult {
        title,
        output: output_text,
        metadata: {
            let mut m = HashMap::new();
            m.insert("exitCode".to_string(), json!(exit_code));
            m.insert("duration".to_string(), json!(duration.as_millis()));
            m.insert("workdir".to_string(), json!(&args.workdir));
            m.insert("command".to_string(), json!(&args.command));
            m
        },
        truncated,
        attachments: Vec::new(),
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}
