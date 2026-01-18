//! Prompt command - runs a single prompt without TUI.

use crate::config::Config;
use crate::permission::PermissionChecker;
use crate::provider::{self, ChatContent, ChatMessage, ContentPart, StreamEvent, ToolDefinition};
use crate::session::{CreateSessionOptions, ModelRef, Session};
use crate::tool::{self, DoomLoopDetector, PendingToolCall, ToolCallTracker, ToolContext};
use anyhow::Result;
use tokio::sync::mpsc;

/// Context for prompt execution
struct PromptContext {
    provider_id: String,
    model_id: String,
    api_key: String,
    model_api_id: String,
    model_api_url: Option<String>,
    max_tokens: u64,
    tool_defs: Vec<ToolDefinition>,
    tool_ctx: ToolContext,
    permission_checker: PermissionChecker,
    format: String,
    system_prompt: String,
}

/// Result of processing a stream
struct StreamResult {
    response_text: String,
    pending_calls: Vec<PendingToolCall>,
    finish_reason: String,
}

/// Execute a single prompt without TUI (with agentic loop)
pub async fn execute(prompt: &str, model: Option<&str>, format: &str) -> Result<()> {
    // Initialize context
    let (ctx, mut session) = initialize_context(model, format).await?;

    // Initialize conversation history
    let mut messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(prompt.to_string()),
    }];

    // Create streaming client
    let client = provider::StreamingClient::new();

    // Agentic loop
    let mut step = 0;
    let max_steps = 20; // Increased from 10 to allow more complex workflows
    let mut doom_detector = DoomLoopDetector::new();

    loop {
        step += 1;
        if step > max_steps {
            if ctx.format == "text" {
                eprintln!(
                    "\n[Warning: Maximum agentic loop steps ({}) reached]",
                    max_steps
                );
            }
            break;
        }

        if ctx.format == "text" && step > 1 {
            eprintln!("\n[Agentic step {}/{}]", step, max_steps);
        }

        // Stream the response
        let rx = create_provider_stream(&client, &ctx, &messages).await?;

        if ctx.format == "text" && step == 1 {
            // Don't print anything for first step
        }

        // Process the stream
        let result = process_stream(rx, &ctx.format).await?;

        // Handle the result
        let should_continue =
            handle_stream_result(&ctx, &mut messages, result, &mut doom_detector).await?;

        if ctx.format == "text" && !should_continue {
            eprintln!("[Agentic loop complete]");
        }

        if !should_continue {
            break;
        }
    }

    // Output and save
    output_result(&messages, step, format);
    save_model_to_session(&mut session, &ctx.provider_id, &ctx.model_id).await;

    Ok(())
}

/// Initialize the prompt context with config, provider, and tools
async fn initialize_context(model: Option<&str>, format: &str) -> Result<(PromptContext, Session)> {
    // Load configuration
    let config = Config::load().await?;

    // Initialize provider registry
    provider::registry().initialize(&config).await?;

    // Create a session
    let session = Session::create(CreateSessionOptions::default()).await?;

    // Resolve model
    let (provider_id, model_id) = resolve_model(model, &session, &config).await?;

    // Get model info
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

    // Create CLI permission handler
    let permission_handler: tool::PermissionHandler = std::sync::Arc::new(move |request| {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        // Spawn async task to check approved rules and handle the request
        let request_clone = request.clone();
        tokio::spawn(async move {
            // 1. Check if this request matches any approved rules
            if crate::permission_state::check_auto_approve(&request_clone).await {
                // Auto-approved - respond immediately
                let _ = response_tx.send(tool::PermissionResponse {
                    id: request_clone.id.clone(),
                    allow: true,
                    scope: tool::PermissionScope::Session,
                });
                return;
            }

            // 2. Not approved - need to ask the user
            // Store response channel for later use
            crate::permission_state::store_response_channel(request_clone.id.clone(), response_tx)
                .await;

            // Store pending request for potential auto-approval
            crate::permission_state::store_pending_request(
                crate::permission_state::PermissionRequestInfo {
                    id: request_clone.id.clone(),
                    permission: request_clone.permission.clone(),
                    patterns: request_clone.patterns.clone(),
                    always: request_clone.always.clone(),
                    metadata: request_clone.metadata.clone(),
                },
            )
            .await;

            // 3. Ask user in blocking thread
            let request_for_blocking = request_clone.clone();
            tokio::task::spawn_blocking(move || {
                use std::io::{self, Write};

                eprintln!("\n[Permission Required]");
                eprintln!("Tool: {}", request_for_blocking.permission);
                eprintln!("Patterns: {:?}", request_for_blocking.patterns);
                eprintln!(
                    "Action: Execute with arguments: {}",
                    serde_json::to_string(&request_for_blocking.metadata).unwrap_or_default()
                );
                eprintln!();
                eprintln!("Options:");
                eprintln!("  y/yes      - Allow once (this request only)");
                eprintln!("  s/session  - Allow for this session (until program restarts)");
                eprintln!("  w/workspace- Allow for this workspace (saved to .opencode/)");
                eprintln!("  g/global   - Allow globally for this user");
                eprintln!("  n/no       - Deny this request");
                eprint!("\nChoice [Y/s/w/g/n]: ");
                let _ = io::stderr().flush();

                let mut input = String::new();
                let _ = io::stdin().read_line(&mut input);

                let answer = input.trim().to_lowercase();

                // Parse the response
                let (allow, scope) = if answer.is_empty() || answer == "y" || answer == "yes" {
                    (true, tool::PermissionScope::Once)
                } else if answer == "s" || answer == "session" {
                    (true, tool::PermissionScope::Session)
                } else if answer == "w" || answer == "workspace" {
                    (true, tool::PermissionScope::Workspace)
                } else if answer == "g" || answer == "global" {
                    (true, tool::PermissionScope::Global)
                } else {
                    // "n", "no", or anything else = deny
                    (false, tool::PermissionScope::Once)
                };

                // Send response via global state handler
                tokio::runtime::Handle::current().block_on(async {
                    crate::permission_state::send_permission_response(
                        request_for_blocking.id,
                        allow,
                        scope,
                    )
                    .await;
                });
            });
        });

        response_rx
    });

    // Create tool context
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let tool_ctx = ToolContext::new("cli-session", "msg-1", "default")
        .with_cwd(cwd.clone())
        .with_root(cwd.clone())
        .with_permission_handler(permission_handler);

    // Generate system prompt
    let system_prompt = crate::session::system::generate(&cwd, &provider_id, &model_id);

    // Get tool definitions
    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<ToolDefinition> = tools
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    Ok((
        PromptContext {
            provider_id,
            model_id,
            api_key,
            model_api_id: model_info.api.id.clone(),
            model_api_url: model_info.api.url.clone(),
            max_tokens: model_info.limit.output,
            tool_defs,
            tool_ctx,
            permission_checker,
            format: format.to_string(),
            system_prompt,
        },
        session,
    ))
}

/// Resolve which model to use based on priority
async fn resolve_model(
    model: Option<&str>,
    session: &Session,
    config: &Config,
) -> Result<(String, String)> {
    if let Some(m) = model {
        // CLI argument takes highest priority
        return provider::parse_model_string(m)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format. Use 'provider/model'"));
    }

    if let Some(session_model) = session.get_model().await {
        // Session model is second priority
        return Ok((session_model.provider_id, session_model.model_id));
    }

    if let Some(configured_model) = config.model.as_ref() {
        // Workspace/global config is third priority
        return provider::parse_model_string(configured_model)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format in config"));
    }

    // Fall back to last used model from global storage
    match crate::storage::global()
        .read::<String>(&["state", "last_model"])
        .await
    {
        Ok(Some(last_model)) => provider::parse_model_string(&last_model)
            .ok_or_else(|| anyhow::anyhow!("Invalid last used model format")),
        Ok(None) | Err(_) => Err(anyhow::anyhow!(
            "No model configured. Set a default model in config or use --model flag"
        )),
    }
}

/// Create a provider-specific stream
async fn create_provider_stream(
    client: &provider::StreamingClient,
    ctx: &PromptContext,
    messages: &[ChatMessage],
) -> Result<mpsc::Receiver<StreamEvent>> {
    match ctx.provider_id.as_str() {
        "anthropic" => {
            client
                .stream_anthropic(
                    &ctx.api_key,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    Some(ctx.system_prompt.clone()),
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        "openai" => {
            let base_url = ctx
                .model_api_url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1");
            client
                .stream_openai(
                    &ctx.api_key,
                    base_url,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    Some(ctx.system_prompt.clone()),
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    &ctx.api_key,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    Some(ctx.system_prompt.clone()),
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", ctx.provider_id)),
    }
}

/// Process stream events and collect results
async fn process_stream(mut rx: mpsc::Receiver<StreamEvent>, format: &str) -> Result<StreamResult> {
    let mut response_text = String::new();
    let mut tool_tracker = ToolCallTracker::new();
    let mut finish_reason = String::new();
    let mut last_printed_newline = false;

    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::TextDelta(text) => {
                handle_text_delta(&text, format, &mut response_text);
                last_printed_newline = false;
            }
            StreamEvent::ReasoningDelta(text) => {
                handle_reasoning_delta(&text, format);
                last_printed_newline = false;
            }
            StreamEvent::ToolCallStart { id, name } => {
                if format == "text" {
                    if !last_printed_newline {
                        println!();
                    }
                    println!("[Calling tool: {}]", name);
                    last_printed_newline = true;
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
                if format == "text" {
                    println!("[Tool call {} ready]", id);
                    last_printed_newline = true;
                }
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                if format == "text" {
                    if !last_printed_newline {
                        println!();
                    }
                    eprintln!("[Tokens: {} in, {} out]", input_tokens, output_tokens);
                    last_printed_newline = true;
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

    Ok(StreamResult {
        response_text,
        pending_calls: tool_tracker.get_all_calls(),
        finish_reason,
    })
}

/// Handle text delta event
fn handle_text_delta(text: &str, format: &str, response_text: &mut String) {
    if format == "text" {
        print!("{}", text);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    response_text.push_str(text);
}

/// Handle reasoning delta event
fn handle_reasoning_delta(text: &str, format: &str) {
    if format == "text" {
        print!("\x1b[2m{}\x1b[0m", text); // dim
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

/// Handle stream result and execute tools if needed
async fn handle_stream_result(
    ctx: &PromptContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    doom_detector: &mut DoomLoopDetector,
) -> Result<bool> {
    let mut assistant_parts: Vec<ContentPart> = Vec::new();

    // Add text if present
    if !result.response_text.is_empty() {
        assistant_parts.push(ContentPart::Text {
            text: result.response_text.clone(),
        });
    }

    // Check if there are tool calls
    // Anthropic may return "end_turn" or "stop" even when tool calls are present
    // So we should execute tools whenever they are present, regardless of finish_reason
    let has_tool_calls = !result.pending_calls.is_empty();
    let should_execute_tools = has_tool_calls;

    if should_execute_tools {
        // Handle tool calls and continue loop
        handle_tool_calls(ctx, messages, result, assistant_parts, doom_detector).await
    } else {
        // No tool calls or final response - add assistant message and exit
        handle_final_response(messages, result, assistant_parts)
    }
}

/// Handle tool calls from the LLM
async fn handle_tool_calls(
    ctx: &PromptContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    mut assistant_parts: Vec<ContentPart>,
    doom_detector: &mut DoomLoopDetector,
) -> Result<bool> {
    // Check for doom loop
    doom_detector.add_calls(&result.pending_calls);

    if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
        if !handle_doom_loop(ctx, &tool_name, &args).await? {
            // User declined doom loop continuation
            // Add assistant message with tool calls but don't execute them
            for call in &result.pending_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));

                assistant_parts.push(ContentPart::ToolUse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: args,
                });
            }

            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: build_chat_content(assistant_parts),
            });

            return Ok(false);
        }
    }

    // Execute all tools - permission checks happen inside tool execution
    // via the ToolContext's permission_handler
    if ctx.format == "text" {
        eprintln!(
            "[Executing {} tool(s) in parallel...]",
            result.pending_calls.len()
        );
    }

    // Add tool use parts to assistant message
    for call in &result.pending_calls {
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
        content: build_chat_content(assistant_parts),
    });

    // Execute tools (permission checks happen inside via ToolContext)
    let tool_results = execute_tools(ctx, result.pending_calls).await;

    // Add tool results to conversation
    let tool_result_msg = tool::build_tool_result_message(tool_results);

    if ctx.format == "text" {
        eprintln!("[Adding tool results to conversation]");
    }

    messages.push(tool_result_msg);

    // Continue the loop
    Ok(true)
}

/// Handle doom loop detection
async fn handle_doom_loop(ctx: &PromptContext, tool_name: &str, args: &str) -> Result<bool> {
    eprintln!("\n[WARNING: Doom loop detected!]");
    eprintln!(
        "[The LLM has called '{}' with identical arguments {} times in a row]",
        tool_name,
        tool::DOOM_LOOP_THRESHOLD
    );
    eprintln!("[Arguments: {}]", args);
    eprintln!("[This may indicate the LLM is stuck.]");

    let allowed = ctx
        .permission_checker
        .check_doom_loop_and_ask_cli(tool_name, args)
        .await?;

    if !allowed {
        eprintln!("[User declined doom loop continuation - stopping execution]");
        return Ok(false);
    }

    eprintln!("[User approved - continuing execution]");
    Ok(true)
}

/// Execute tools using ToolContext (which handles permissions internally)
async fn execute_tools(ctx: &PromptContext, calls: Vec<PendingToolCall>) -> Vec<ContentPart> {
    if ctx.format == "text" {
        eprintln!("[Executing {} tool(s) in parallel...]", calls.len());
    }

    let tool_results = tool::execute_all_tools_parallel(calls, &ctx.tool_ctx).await;

    // Show tool results in text format
    if ctx.format == "text" {
        print_tool_results(&tool_results);
        eprintln!("[Tool execution complete]");
    }

    tool_results
}

/// Print tool results to console
fn print_tool_results(results: &[ContentPart]) {
    for result in results {
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
            eprintln!("[Tool {} result: {}]", tool_use_id, status);

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(title) = parsed.get("title") {
                    if let Some(title_str) = title.as_str() {
                        eprintln!("  Title: {}", title_str);
                    }
                }
                if let Some(output) = parsed.get("output") {
                    if let Some(output_str) = output.as_str() {
                        // Use char-boundary-safe truncation
                        let preview = truncate_str_safe(output_str, 200);
                        eprintln!("  Output: {}", preview);
                    }
                }
            }
        }
    }
}

/// Safely truncate a string at character boundaries
fn truncate_str_safe(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Build ChatContent from parts
fn build_chat_content(parts: Vec<ContentPart>) -> ChatContent {
    if parts.len() == 1 {
        if let ContentPart::Text { text } = &parts[0] {
            return ChatContent::Text(text.clone());
        }
    }
    ChatContent::Parts(parts)
}

/// Handle final response when no tool calls
fn handle_final_response(
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    assistant_parts: Vec<ContentPart>,
) -> Result<bool> {
    // Add assistant message if we have any content
    if !assistant_parts.is_empty() {
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: build_chat_content(assistant_parts),
        });
    } else if !result.response_text.is_empty() {
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Text(result.response_text),
        });
    }

    if result.finish_reason == "tool_calls" || result.finish_reason == "tool_use" {
        eprintln!("[Warning: LLM indicated tool_calls but no tools found or executed]");
    }

    println!();
    Ok(false)
}

/// Output result in requested format
fn output_result(messages: &[ChatMessage], step: i32, format: &str) {
    if format == "json" {
        let output = serde_json::json!({
            "messages": messages,
            "steps": step,
        });
        if let Ok(json) = serde_json::to_string_pretty(&output) {
            println!("{}", json);
        }
    }
    // For "text" and "markdown", output was already printed during streaming
}

/// Save model to session and global storage
async fn save_model_to_session(session: &mut Session, provider_id: &str, model_id: &str) {
    let model_ref = ModelRef {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    };

    if let Err(e) = session
        .set_model(&session.project_id.clone(), model_ref)
        .await
    {
        tracing::warn!("Failed to save model to session: {}", e);
    }

    // Save last used model to global storage
    let model_string = format!("{}/{}", provider_id, model_id);
    if let Err(e) = crate::storage::global()
        .write(&["state", "last_model"], &model_string)
        .await
    {
        tracing::warn!("Failed to save last used model: {}", e);
    }
}
