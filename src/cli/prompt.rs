//! Prompt command - runs a single prompt without TUI.

use crate::config::Config;
use crate::provider::{self, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::tool;
use anyhow::Result;

/// Execute a single prompt without TUI
pub async fn execute(prompt: &str, model: Option<&str>, format: &str) -> Result<()> {
    // Load configuration
    let config = Config::load().await?;

    // Initialize provider registry
    provider::registry().initialize(&config).await?;

    // Get model to use
    let (provider_id, model_id) = if let Some(m) = model {
        provider::parse_model_string(m)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format. Use 'provider/model'"))?
    } else {
        provider::registry()
            .default_model(&config)
            .await
            .ok_or_else(|| anyhow::anyhow!("No default model configured"))?
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

    // Create a session
    let _session = Session::create(CreateSessionOptions::default()).await?;

    // Create messages
    let _cwd = std::env::current_dir()?.to_string_lossy().to_string();

    let messages = vec![provider::ChatMessage {
        role: "user".to_string(),
        content: provider::ChatContent::Text(prompt.to_string()),
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

    // Stream the response
    let mut rx = match provider_id.as_str() {
        "anthropic" => {
            client
                .stream_anthropic(
                    &api_key,
                    &model_info.api.id,
                    messages,
                    None, // system prompt
                    tool_defs,
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
                    messages,
                    tool_defs,
                    model_info.limit.output,
                )
                .await?
        }
        "copilot" => {
            client
                .stream_copilot(
                    &api_key,
                    &model_info.api.id,
                    messages,
                    tool_defs,
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
    let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
    let mut current_tool_args = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::TextDelta(text) => {
                if format == "text" {
                    print!("{}", text);
                }
                response_text.push_str(&text);
            }
            StreamEvent::ReasoningDelta(text) => {
                if format == "text" {
                    // Show reasoning in a different style
                    print!("\x1b[2m{}\x1b[0m", text); // dim
                }
            }
            StreamEvent::ToolCallStart { id, name } => {
                if format == "text" {
                    eprintln!("\n[Tool: {}]", name);
                }
                tool_calls.push((id, name, String::new()));
                current_tool_args.clear();
            }
            StreamEvent::ToolCallDelta {
                id: _,
                arguments_delta,
            } => {
                current_tool_args.push_str(&arguments_delta);
            }
            StreamEvent::ToolCallEnd { id } => {
                if let Some((_, _, args)) = tool_calls.iter_mut().find(|(i, _, _)| *i == id) {
                    *args = current_tool_args.clone();
                }
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                if format == "text" {
                    eprintln!("\n[Tokens: {} in, {} out]", input_tokens, output_tokens);
                }
            }
            StreamEvent::Done { finish_reason: _ } => {
                if format == "text" {
                    println!();
                }
            }
            StreamEvent::Error(err) => {
                eprintln!("\nError: {}", err);
                return Err(anyhow::anyhow!(err));
            }
        }
    }

    // Output in requested format
    match format {
        "json" => {
            let output = serde_json::json!({
                "response": response_text,
                "tool_calls": tool_calls.iter().map(|(id, name, args)| {
                    serde_json::json!({
                        "id": id,
                        "name": name,
                        "arguments": args
                    })
                }).collect::<Vec<_>>()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        "markdown" => {
            println!("{}", response_text);
            if !tool_calls.is_empty() {
                println!("\n## Tool Calls\n");
                for (_id, name, args) in &tool_calls {
                    println!("### {}\n", name);
                    println!("```json\n{}\n```\n", args);
                }
            }
        }
        _ => {
            // Already printed during streaming
        }
    }

    Ok(())
}
