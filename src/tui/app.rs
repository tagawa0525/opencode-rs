//! Main TUI event loop and entry point.
//!
//! This module contains the main run function and event loop.
//! The App state is defined in state.rs.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use super::input::{key_to_action, Action};
use super::llm_streaming::{stream_response, stream_response_agentic};
use super::oauth_flow::{start_copilot_oauth_flow, start_openai_oauth_flow};
use super::ui;

// Re-export App for backward compatibility
pub use super::state::App;

// Re-export types for backward compatibility
pub use super::types::{
    AppEvent, AutocompleteState, CommandItem, DialogState, DialogType, DisplayMessage, MessagePart,
    PermissionRequest, SelectItem,
};
use crate::config::Config;
use crate::provider::{self, Provider, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::slash_command::{parser::ParsedCommand, CommandAction, CommandContext, CommandOutput};
use crate::tool;

/// Run the TUI application
pub async fn run(initial_prompt: Option<String>, model: Option<String>) -> Result<()> {
    // Check if we're running in a TTY
    if !atty::is(atty::Stream::Stdout) {
        anyhow::bail!(
            "This command requires a TTY (terminal). Please run in an interactive terminal,\n\
            or use the 'prompt' command instead for non-interactive usage:\n  \
            opencode prompt \"your message here\""
        );
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(model).await?;

    // If no model configured, open provider/model selector
    if !app.model_configured {
        if app.available_providers.is_empty() {
            // No providers with API keys - show provider selector
            app.open_provider_selector();
        } else {
            // Providers available - show model selector
            app.open_model_selector();
        }
    }

    // If there's an initial prompt, set it as input
    if let Some(prompt) = initial_prompt {
        app.input = prompt;
        app.cursor_position = app.input.len();
    }

    // Event channel for async processing
    let (event_tx, mut event_rx) = mpsc::channel::<AppEvent>(100);

    // Run event loop
    let result = run_app(&mut terminal, &mut app, event_tx, &mut event_rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main event loop
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_tx: mpsc::Sender<AppEvent>,
    event_rx: &mut mpsc::Receiver<AppEvent>,
) -> Result<()> {
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = std::time::Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| ui::render(f, app))?;

        // Handle events
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    // Handle autocomplete input if autocomplete is open
                    if app.autocomplete.is_some() {
                        match key.code {
                            KeyCode::Up => {
                                if let Some(autocomplete) = &mut app.autocomplete {
                                    autocomplete.move_up();
                                }
                                continue;
                            }
                            KeyCode::Down => {
                                if let Some(autocomplete) = &mut app.autocomplete {
                                    autocomplete.move_down();
                                }
                                continue;
                            }
                            KeyCode::Enter | KeyCode::Tab => {
                                // Execute the selected command immediately
                                if let Some(command_name) = app.insert_autocomplete_selection() {
                                    // Execute slash command
                                    let ctx = CommandContext {
                                        session_id: app
                                            .session
                                            .as_ref()
                                            .map(|s| s.id.clone())
                                            .unwrap_or_default(),
                                        cwd: std::env::current_dir()
                                            .ok()
                                            .and_then(|p| p.to_str().map(String::from))
                                            .unwrap_or_else(|| ".".to_string()),
                                        root: std::env::current_dir()
                                            .ok()
                                            .and_then(|p| p.to_str().map(String::from))
                                            .unwrap_or_else(|| ".".to_string()),
                                        extra: Default::default(),
                                    };

                                    let registry = app.command_registry.clone();
                                    match registry.execute(&command_name, "", &ctx).await {
                                        Ok(output) => {
                                            handle_command_output(
                                                app,
                                                &command_name,
                                                output,
                                                event_tx.clone(),
                                            )
                                            .await?;
                                        }
                                        Err(e) => {
                                            app.add_message("system", &format!("Error: {}", e));
                                        }
                                    }
                                }
                                continue;
                            }
                            KeyCode::Esc => {
                                app.hide_autocomplete();
                                continue;
                            }
                            _ => {
                                // Let the normal input handling process the key
                                // but we'll update autocomplete after
                            }
                        }
                    }

                    // Handle dialog input if dialog is open
                    if app.dialog.is_some() {
                        handle_dialog_input(app, key, event_tx.clone()).await?;
                    } else {
                        let action = key_to_action(key);

                        // Check for model selector keybind (Ctrl+M)
                        if key.code == KeyCode::Char('m') && key.modifiers == KeyModifiers::CONTROL
                        {
                            app.open_model_selector();
                            continue;
                        }

                        // Check for provider selector keybind (Ctrl+P)
                        if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL
                        {
                            app.open_provider_selector();
                            continue;
                        }

                        if action == Action::Submit && !app.is_processing {
                            // Check if model is configured
                            if !app.is_ready() {
                                app.open_model_selector();
                                continue;
                            }

                            if let Some(input) = app.take_input() {
                                // Check if input is just "/" - show help for slash commands
                                if input.trim() == "/" {
                                    // Show available slash commands
                                    let commands = app.command_registry.list().await;
                                    let mut help_text =
                                        String::from("Available slash commands:\n\n");
                                    for cmd in commands {
                                        help_text.push_str(&format!(
                                            "  /{} - {}\n",
                                            cmd.name, cmd.description
                                        ));
                                    }
                                    help_text.push_str("\nType /help for more information.");
                                    app.add_message("system", &help_text);
                                    continue;
                                }

                                // Check if this is a slash command
                                if let Some(parsed) = ParsedCommand::parse(&input) {
                                    // Execute slash command
                                    let ctx = CommandContext {
                                        session_id: app
                                            .session
                                            .as_ref()
                                            .map(|s| s.id.clone())
                                            .unwrap_or_default(),
                                        cwd: std::env::current_dir()
                                            .ok()
                                            .and_then(|p| p.to_str().map(String::from))
                                            .unwrap_or_else(|| ".".to_string()),
                                        root: std::env::current_dir()
                                            .ok()
                                            .and_then(|p| p.to_str().map(String::from))
                                            .unwrap_or_else(|| ".".to_string()),
                                        extra: Default::default(),
                                    };

                                    let registry = app.command_registry.clone();
                                    match registry.execute(&parsed.name, &parsed.args, &ctx).await {
                                        Ok(output) => {
                                            handle_command_output(
                                                app,
                                                &parsed.name,
                                                output,
                                                event_tx.clone(),
                                            )
                                            .await?;
                                        }
                                        Err(e) => {
                                            app.add_message("system", &format!("Error: {}", e));
                                        }
                                    }
                                    continue;
                                }

                                // Normal user message (not a slash command)
                                // Add user message
                                app.add_message("user", &input);
                                app.is_processing = true;
                                app.status = "Processing".to_string();

                                // Add empty assistant message
                                app.add_message("assistant", "");

                                // Start agentic loop
                                let tx = event_tx.clone();
                                let provider_id = app.provider_id.clone();
                                let model_id = app.model_id.clone();
                                let prompt = input.clone();

                                tokio::spawn(async move {
                                    if let Err(e) = stream_response_agentic(
                                        provider_id,
                                        model_id,
                                        prompt,
                                        tx.clone(),
                                    )
                                    .await
                                    {
                                        let _ = tx.send(AppEvent::StreamError(e.to_string())).await;
                                    }
                                });
                            }
                        } else if action == Action::Cancel && app.is_processing {
                            // Cancel processing
                            app.is_processing = false;
                            app.status = "Ready".to_string();
                        } else {
                            app.handle_action(action);
                            // Update autocomplete after input changes
                            app.update_autocomplete().await;
                        }
                    }
                }
                _ => {}
            }
        }

        // Process async events
        while let Ok(event) = event_rx.try_recv() {
            match event {
                AppEvent::StreamDelta(text) => {
                    app.append_to_assistant(&text);
                }
                AppEvent::StreamDone => {
                    app.is_processing = false;
                    app.status = "Ready".to_string();
                }
                AppEvent::StreamError(err) => {
                    app.is_processing = false;
                    app.status = "Error".to_string();
                    app.add_message("system", &format!("Error: {}", err));
                }
                AppEvent::ToolCall(name, id) => {
                    app.append_to_assistant(&format!("\n[Calling tool: {}]\n", name));
                    app.add_tool_call(&id, &name, "");
                }
                AppEvent::DeviceCodeReceived {
                    user_code,
                    verification_uri,
                    device_code,
                    interval: _,
                } => {
                    app.show_device_code(&user_code, &verification_uri, &device_code);
                    // Try to open browser
                    let _ = open::that(&verification_uri);
                }
                AppEvent::OAuthSuccess { provider_id } => {
                    // Re-initialize registry to pick up new credentials
                    let config = Config::load().await?;
                    provider::registry().initialize(&config).await?;
                    app.all_providers = provider::registry().list().await;
                    app.available_providers = provider::registry().list_available().await;
                    app.close_dialog();
                    app.add_message(
                        "system",
                        &format!("Successfully connected to {}!", provider_id),
                    );
                    app.open_model_selector();
                }
                AppEvent::OAuthError(err) => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.message = Some(format!("Error: {}", err));
                    }
                }
                AppEvent::ToolResult {
                    id,
                    output,
                    is_error,
                } => {
                    // Show tool result in messages
                    let status = if is_error { "ERROR" } else { "OK" };
                    let mut display_output = output.clone();

                    // Try to parse as JSON and extract meaningful info
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
                        if let Some(title) = parsed.get("title").and_then(|v| v.as_str()) {
                            display_output = title.to_string();
                        }
                    }

                    // Limit output length for display
                    if display_output.len() > 200 {
                        display_output = format!("{}...", &display_output[..200]);
                    }

                    app.append_to_assistant(&format!(
                        "\n[Tool {} result: {}] {}\n",
                        id, status, display_output
                    ));
                    app.add_tool_result(&id, &output, is_error);
                }
                AppEvent::PermissionRequested(request) => {
                    // Show permission dialog
                    app.show_permission_request(request);
                }
                AppEvent::PermissionResponse { id, allow, always } => {
                    // Handle permission response
                    // TODO: Send response back to agentic loop
                    // For now, just log it
                    if allow {
                        if always {
                            app.status = format!("Permission granted (always): {}", id);
                        } else {
                            app.status = format!("Permission granted (once): {}", id);
                        }
                    } else {
                        app.status = format!("Permission denied: {}", id);
                    }
                }
            }
        }

        // Tick for animations
        if last_tick.elapsed() >= tick_rate {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
            last_tick = std::time::Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handle input when a dialog is open
async fn handle_dialog_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    let dialog_type = app.dialog.as_ref().map(|d| d.dialog_type.clone());

    match dialog_type {
        Some(DialogType::ModelSelector) | Some(DialogType::ProviderSelector) => {
            match key.code {
                KeyCode::Esc => {
                    // Close dialog, but if model not configured, quit
                    if !app.model_configured
                        && app.dialog.as_ref().map(|d| &d.dialog_type)
                            == Some(&DialogType::ModelSelector)
                    {
                        app.should_quit = true;
                    }
                    app.close_dialog();
                }
                KeyCode::Enter => {
                    // Select item
                    if let Some(dialog) = &app.dialog {
                        if let Some(item) = dialog.selected_item() {
                            let item_id = item.id.clone();
                            let dialog_type = dialog.dialog_type.clone();

                            match dialog_type {
                                DialogType::ModelSelector => {
                                    // Parse provider/model from item_id
                                    if let Some((provider_id, model_id)) =
                                        provider::parse_model_string(&item_id)
                                    {
                                        app.set_model(&provider_id, &model_id).await?;
                                    }
                                }
                                DialogType::ProviderSelector => {
                                    let provider_id = item_id.clone();
                                    // Check if provider already has a key
                                    let has_key = app
                                        .all_providers
                                        .iter()
                                        .find(|p| p.id == provider_id)
                                        .map(|p| p.key.is_some())
                                        .unwrap_or(false);

                                    if has_key {
                                        // Provider connected, open model selector
                                        app.close_dialog();
                                        app.open_model_selector();
                                    } else {
                                        // Show auth method selector for providers with OAuth
                                        app.open_auth_method_selector(&provider_id);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                KeyCode::Up => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.move_up();
                    }
                }
                KeyCode::Down => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.move_down();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.search_query.push(c);
                        dialog.update_filter();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.search_query.pop();
                        dialog.update_filter();
                    }
                }
                _ => {}
            }
        }
        Some(DialogType::ApiKeyInput) => {
            match key.code {
                KeyCode::Esc => {
                    // Go back to provider selector
                    app.open_provider_selector();
                }
                KeyCode::Enter => {
                    // Save API key
                    if let Some(dialog) = &app.dialog {
                        let api_key = dialog.input_value.clone();
                        let provider_id = dialog
                            .items
                            .first()
                            .map(|i| i.id.clone())
                            .unwrap_or_default();
                        let env_var = dialog
                            .items
                            .first()
                            .map(|i| i.label.clone())
                            .unwrap_or_default();

                        if !api_key.is_empty() {
                            // Set environment variable for current session
                            std::env::set_var(&env_var, &api_key);

                            // Re-initialize registry
                            let config = Config::load().await?;
                            provider::registry().initialize(&config).await?;

                            // Update cached providers
                            app.all_providers = provider::registry().list().await;
                            app.available_providers = provider::registry().list_available().await;

                            // Close dialog and open model selector
                            app.close_dialog();
                            app.open_model_selector();

                            // Save to auth file
                            if let Err(e) = crate::auth::save_api_key(&provider_id, &api_key).await
                            {
                                // Log error but don't fail
                                eprintln!("Warning: Failed to save API key: {}", e);
                            }
                        }
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.input_value.push(c);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.input_value.pop();
                    }
                }
                _ => {}
            }
        }
        Some(DialogType::AuthMethodSelector) => {
            match key.code {
                KeyCode::Esc => {
                    app.open_provider_selector();
                }
                KeyCode::Enter => {
                    if let Some(dialog) = &app.dialog {
                        if let Some(item) = dialog.selected_item() {
                            let auth_method = item.id.clone();
                            let provider_id = item.provider_id.clone().unwrap_or_default();

                            match auth_method.as_str() {
                                "oauth" => {
                                    match provider_id.as_str() {
                                        "copilot" => {
                                            app.start_copilot_oauth();
                                            // Start OAuth flow in background
                                            let tx = event_tx.clone();
                                            tokio::spawn(async move {
                                                start_copilot_oauth_flow(tx).await;
                                            });
                                        }
                                        "openai" => {
                                            app.start_openai_oauth();
                                            // Start OAuth flow in background
                                            let tx = event_tx.clone();
                                            tokio::spawn(async move {
                                                start_openai_oauth_flow(tx).await;
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                                "api_key" => {
                                    app.open_api_key_input(&provider_id);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                KeyCode::Up => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.move_up();
                    }
                }
                KeyCode::Down => {
                    if let Some(dialog) = &mut app.dialog {
                        dialog.move_down();
                    }
                }
                _ => {}
            }
        }
        Some(DialogType::OAuthDeviceCode) | Some(DialogType::OAuthWaiting) => {
            // Only allow Esc to cancel
            if key.code == KeyCode::Esc {
                app.open_provider_selector();
            }
        }
        Some(DialogType::PermissionRequest) => {
            // Handle permission dialog input
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // Allow once
                    if let Some(dialog) = &app.dialog {
                        if let Some(req) = &dialog.permission_request {
                            let id = req.id.clone();
                            app.close_dialog();
                            let _ = event_tx
                                .send(AppEvent::PermissionResponse {
                                    id,
                                    allow: true,
                                    always: false,
                                })
                                .await;
                        }
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    // Allow always
                    if let Some(dialog) = &app.dialog {
                        if let Some(req) = &dialog.permission_request {
                            let id = req.id.clone();
                            app.close_dialog();
                            let _ = event_tx
                                .send(AppEvent::PermissionResponse {
                                    id,
                                    allow: true,
                                    always: true,
                                })
                                .await;
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // Reject
                    if let Some(dialog) = &app.dialog {
                        if let Some(req) = &dialog.permission_request {
                            let id = req.id.clone();
                            app.close_dialog();
                            let _ = event_tx
                                .send(AppEvent::PermissionResponse {
                                    id,
                                    allow: false,
                                    always: false,
                                })
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {
            if key.code == KeyCode::Esc {
                app.close_dialog();
            }
        }
    }

    Ok(())
}

/// Handle command output
async fn handle_command_output(
    app: &mut App,
    command_name: &str,
    output: CommandOutput,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Handle special actions
    if let Some(action) = &output.action {
        match action {
            CommandAction::OpenModelSelector => {
                app.open_model_selector();
                return Ok(());
            }
            CommandAction::OpenAgentSelector => {
                // TODO: Implement agent selector
                app.add_message("system", "Agent selector not yet implemented");
                return Ok(());
            }
            CommandAction::OpenSessionList => {
                // TODO: Implement session list
                app.add_message("system", "Session list not yet implemented");
                return Ok(());
            }
            CommandAction::NewSession => {
                // Create new session
                match Session::create(CreateSessionOptions::default()).await {
                    Ok(session) => {
                        app.session_title = session.title.clone();
                        app.session_slug = session.slug.clone();
                        app.session = Some(session);
                        app.messages.clear();
                        app.total_cost = 0.0;
                        app.total_tokens = 0;
                        app.status = "Session cleared".to_string();
                    }
                    Err(e) => {
                        app.status = format!("Error creating session: {}", e);
                    }
                }
                return Ok(());
            }
        }
    }

    // Handle special commands
    if command_name == "clear" || command_name == "new" {
        // Create new session
        match Session::create(CreateSessionOptions::default()).await {
            Ok(session) => {
                app.session_title = session.title.clone();
                app.session_slug = session.slug.clone();
                app.session = Some(session);
                app.messages.clear();
                app.total_cost = 0.0;
                app.total_tokens = 0;
                app.status = "Session cleared".to_string();
            }
            Err(e) => {
                app.status = format!("Error creating session: {}", e);
            }
        }
        return Ok(());
    }

    // Handle model switch
    if let Some(model) = &output.model {
        if let Some((provider_id, model_id)) = provider::parse_model_string(model) {
            app.provider_id = provider_id.clone();
            app.model_id = model_id.clone();
            app.model_display = format!("{}/{}", provider_id, model_id);
            app.model_configured = true;
            app.status = format!("Switched to model: {}", model);
        }
        return Ok(());
    }

    // Handle agent switch
    if let Some(_agent) = &output.agent {
        // TODO: Implement agent switching
        app.status = "Agent switching not yet implemented".to_string();
        return Ok(());
    }

    // Display command output if not empty
    if !output.text.is_empty() {
        app.add_message("system", &output.text);
    }

    // If the command wants to submit to LLM, do it
    if output.submit_to_llm {
        app.is_processing = true;
        app.status = "Processing".to_string();

        // Add empty assistant message
        app.add_message("assistant", "");

        // Start streaming
        let provider_id = app.provider_id.clone();
        let model_id = app.model_id.clone();
        let prompt = output.text.clone();

        tokio::spawn(async move {
            match stream_response(&provider_id, &model_id, &prompt).await {
                Ok(mut rx) => {
                    while let Some(event) = rx.recv().await {
                        match event {
                            StreamEvent::TextDelta(text) => {
                                let _ = event_tx.send(AppEvent::StreamDelta(text)).await;
                            }
                            StreamEvent::Done { .. } => {
                                let _ = event_tx.send(AppEvent::StreamDone).await;
                            }
                            StreamEvent::Error(err) => {
                                let _ = event_tx.send(AppEvent::StreamError(err)).await;
                            }
                            StreamEvent::ToolCallStart { name, .. } => {
                                let _ =
                                    event_tx.send(AppEvent::ToolCall(name, String::new())).await;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    let _ = event_tx.send(AppEvent::StreamError(e.to_string())).await;
                }
            }
        });
    }

    Ok(())
}
