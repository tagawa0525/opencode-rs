//! Prompt command - runs a single prompt without TUI.

use crate::config::Config;
use crate::permission::PermissionChecker;
use crate::provider::{self, ChatContent, ChatMessage, ContentPart, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::tool::{self, DoomLoopDetector, ToolCallTracker, ToolContext};
use anyhow::Result;

/// Execute a single prompt without TUI (with agentic loop)
pub async fn execute(prompt: &str, model: Option<&str>, format: &str) -> Result<()> {
    // Load configuration
    let config = Config::load().await?;

    // Initialize provider registry
    provider::registry().initialize(&config).await?;

    // Create a session first
    let mut session = Session::create(CreateSessionOptions::default()).await?;

    // Get model to use with priority: CLI arg > Session > Workspace config > Global config > Last used
    let (provider_id, model_id) = if let Some(m) = model {
        // CLI argument takes highest priority
        provider::parse_model_string(m)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format. Use 'provider/model'"))?
    } else if let Some(session_model) = session.get_model().await {
        // Session model is second priority
        (session_model.provider_id, session_model.model_id)
    } else if let Some(configured_model) = config.model.as_ref() {
        // Workspace/global config is third priority
        provider::parse_model_string(configured_model)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format in config"))?
    } else {
        // Fall back to last used model from global storage
        match crate::storage::global()
            .read::<String>(&["state", "last_model"])
            .await
        {
            Ok(Some(last_model)) => provider::parse_model_string(&last_model)
                .ok_or_else(|| anyhow::anyhow!("Invalid last used model format"))?,
            Ok(None) | Err(_) => {
                return Err(anyhow::anyhow!(
                    "No model configured. Set a default model in config or use --model flag"
                ))
            }
        }
    };

    // Get the model info
    let model_info = provider::registry()
        .get_model(&provider_id, &model_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}/{}", provider_id, model_id))?;

    // Get API key
    let provider_info = provider::registry()
        .get(&provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let api_key = provider_info
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    // Create permission checker
    let permission_checker = PermissionChecker::from_config(&config);

    // Create tool context
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let tool_ctx = ToolContext::new("cli-session", "msg-1", "default")
        .with_cwd(cwd.clone())
        .with_root(cwd);

    // Initialize conversation history
    let mut messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(prompt.to_string()),
    }];

    // Get tool definitions
    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<provider::ToolDefinition> = tools
        .into_iter()
        .map(|t| provider::ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    // Create streaming client
    let client = provider::StreamingClient::new();

    // Agentic loop - continue until LLM finishes without tool calls
    let mut step = 0;
    let max_steps = 10; // Prevent infinite loops
    let mut doom_detector = DoomLoopDetector::new();

    loop {
        step += 1;
        if step > max_steps {
            eprintln!("\n[Warning: Maximum agentic loop steps reached]");
            break;
        }

        if format == "text" && step > 1 {
            eprintln!("\n[Agentic step {}]", step);
        }

        // Stream the response
        let mut rx = match provider_id.as_str() {
            "anthropic" => {
                client
                    .stream_anthropic(
                        &api_key,
                        &model_info.api.id,
                        messages.clone(),
                        None, // system prompt
                        tool_defs.clone(),
                        model_info.limit.output,
                    )
                    .await?
            }
            "openai" => {
                let base_url = model_info
                    .api
                    .url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1");
                client
                    .stream_openai(
                        &api_key,
                        base_url,
                        &model_info.api.id,
                        messages.clone(),
                        tool_defs.clone(),
                        model_info.limit.output,
                    )
                    .await?
            }
            "copilot" => {
                client
                    .stream_copilot(
                        &api_key,
                        &model_info.api.id,
                        messages.clone(),
                        tool_defs.clone(),
                        model_info.limit.output,
                    )
                    .await?
            }
            _ => {
                anyhow::bail!("Unsupported provider: {}", provider_id);
            }
        };

        // Collect response
        let mut response_text = String::new();
        let mut tool_tracker = ToolCallTracker::new();
        let mut finish_reason = String::new();
        let mut assistant_parts: Vec<ContentPart> = Vec::new();

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(text) => {
                    if format == "text" {
                        print!("{}", text);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                    response_text.push_str(&text);
                }
                StreamEvent::ReasoningDelta(text) => {
                    if format == "text" {
                        // Show reasoning in a different style
                        print!("\x1b[2m{}\x1b[0m", text); // dim
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                }
                StreamEvent::ToolCallStart { id, name } => {
                    if format == "text" {
                        println!("\n[Calling tool: {}]", name);
                    }
                    tool_tracker.start_call(id, name);
                }
                StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    tool_tracker.add_arguments(&id, &arguments_delta);
                }
                StreamEvent::ToolCallEnd { id } => {
                    // Tool call is complete, will be executed after stream ends
                    if format == "text" {
                        println!("[Tool call {} complete]", id);
                    }
                }
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    if format == "text" {
                        eprintln!("[Tokens: {} in, {} out]", input_tokens, output_tokens);
                    }
                }
                StreamEvent::Done {
                    finish_reason: reason,
                } => {
                    finish_reason = reason;
                }
                StreamEvent::Error(err) => {
                    eprintln!("\nError: {}", err);
                    return Err(anyhow::anyhow!(err));
                }
                _ => {}
            }
        }

        // Add assistant response to conversation history
        if !response_text.is_empty() {
            assistant_parts.push(ContentPart::Text {
                text: response_text.clone(),
            });
        }

        // Check if there are tool calls to execute
        let pending_calls = tool_tracker.get_all_calls();

        if !pending_calls.is_empty() {
            // Check for doom loop before executing tools
            doom_detector.add_calls(&pending_calls);

            if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
                eprintln!("\n[WARNING: Doom loop detected!]");
                eprintln!(
                    "[The LLM has called '{}' with identical arguments {} times in a row]",
                    tool_name,
                    tool::DOOM_LOOP_THRESHOLD
                );
                eprintln!("[Arguments: {}]", args);
                eprintln!("[This may indicate the LLM is stuck.]");

                // Check permission for doom loop continuation
                let allowed = permission_checker
                    .check_doom_loop_and_ask_cli(&tool_name, &args)
                    .await?;

                if !allowed {
                    eprintln!("[User declined doom loop continuation - stopping execution]");
                    break;
                }

                eprintln!("[User approved - continuing execution]");
            }

            // Check permissions for each tool call
            let mut approved_calls = Vec::new();
            for call in &pending_calls {
                let allowed = permission_checker
                    .check_and_ask_cli(&call.name, &call.arguments)
                    .await?;

                if allowed {
                    approved_calls.push(call.clone());
                } else {
                    eprintln!("[User denied execution of tool: {}]", call.name);
                }
            }

            if approved_calls.is_empty() {
                eprintln!("[No tools approved - stopping execution]");
                break;
            }

            // Add tool use parts to assistant message (only approved)
            for call in &approved_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));

                assistant_parts.push(ContentPart::ToolUse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: args,
                });
            }

            // Add assistant message with tool calls
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: if assistant_parts.len() == 1 {
                    match &assistant_parts[0] {
                        ContentPart::Text { text } => ChatContent::Text(text.clone()),
                        _ => ChatContent::Parts(assistant_parts.clone()),
                    }
                } else {
                    ChatContent::Parts(assistant_parts)
                },
            });

            // Execute tools (in parallel for better performance)
            if format == "text" {
                println!(
                    "\n[Executing {} tool(s) in parallel...]",
                    approved_calls.len()
                );
            }

            let tool_results = tool::execute_all_tools_parallel(approved_calls, &tool_ctx).await;

            // Show tool results in text format
            if format == "text" {
                for result in &tool_results {
                    if let ContentPart::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = result
                    {
                        let status = if is_error.unwrap_or(false) {
                            "ERROR"
                        } else {
                            "OK"
                        };
                        println!("[Tool {} result: {}]", tool_use_id, status);
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                            if let Some(title) = parsed.get("title") {
                                println!("  {}", title);
                            }
                            if let Some(output) = parsed.get("output") {
                                if let Some(output_str) = output.as_str() {
                                    // Show first 200 chars
                                    let preview = if output_str.len() > 200 {
                                        format!("{}...", &output_str[..200])
                                    } else {
                                        output_str.to_string()
                                    };
                                    println!("  {}", preview);
                                }
                            }
                        }
                    }
                }
            }

            // Add tool results to conversation
            messages.push(tool::build_tool_result_message(tool_results));

            // Continue the loop to get next LLM response
            continue;
        } else {
            // No tool calls - add final assistant message and exit loop
            if !response_text.is_empty() {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: ChatContent::Text(response_text),
                });
            }

            if format == "text" {
                println!();
            }

            // Check finish reason
            if finish_reason == "tool_calls" {
                eprintln!("[Warning: LLM indicated tool_calls but no tools found]");
            }

            break;
        }
    }

    // Output final result in requested format
    match format {
        "json" => {
            let output = serde_json::json!({
                "messages": messages,
                "steps": step,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        "markdown" => {
            // Already printed during streaming
        }
        _ => {
            // Already printed during streaming
        }
    }

    // Save model to session
    let model_ref = crate::session::ModelRef {
        provider_id: provider_id.clone(),
        model_id: model_id.clone(),
    };
    // Note: We need to make session mutable and keep it in scope
    if let Err(e) = session
        .set_model(&session.project_id.clone(), model_ref)
        .await
    {
        tracing::warn!("Failed to save model to session: {}", e);
    }

    // Save last used model to global storage (fallback)
    let model_string = format!("{}/{}", provider_id, model_id);
    if let Err(e) = crate::storage::global()
        .write(&["state", "last_model"], &model_string)
        .await
    {
        tracing::warn!("Failed to save last used model: {}", e);
    }

    Ok(())
}
