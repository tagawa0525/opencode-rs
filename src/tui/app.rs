//! Main TUI event loop and entry point.
//!
//! This module contains the main run function and event loop.
//! The App state is defined in state.rs.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use super::command_handler::handle_command_output;
use super::dialog::handle_dialog_input;
use super::input::{key_to_action, Action};
use super::llm_streaming::stream_response_agentic;
use super::ui;

// Re-export App for backward compatibility
pub use super::state::App;

// Re-export types for backward compatibility
pub use super::types::{
    AppEvent, AutocompleteState,
};
use crate::config::Config;
use crate::provider;
use crate::slash_command::{parser::ParsedCommand, CommandContext};

/// Run the TUI application
pub async fn run(initial_prompt: Option<String>, model: Option<String>) -> Result<()> {
    // Check if we're running in a TTY
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
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
            app.open_provider_selector();
        } else {
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

/// Create a command context from the current app state
fn create_command_context(app: &App) -> CommandContext {
    CommandContext {
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
    }
}

/// Handle autocomplete key events
/// Returns true if the event was handled and should not be processed further
async fn handle_autocomplete_input(
    app: &mut App,
    key: KeyEvent,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<bool> {
    if app.autocomplete.is_none() {
        return Ok(false);
    }

    match key.code {
        KeyCode::Up => {
            if let Some(autocomplete) = &mut app.autocomplete {
                autocomplete.move_up();
            }
            Ok(true)
        }
        KeyCode::Down => {
            if let Some(autocomplete) = &mut app.autocomplete {
                autocomplete.move_down();
            }
            Ok(true)
        }
        KeyCode::Enter | KeyCode::Tab => {
            if let Some(command_name) = app.insert_autocomplete_selection() {
                let ctx = create_command_context(app);
                let registry = app.command_registry.clone();
                match registry.execute(&command_name, "", &ctx).await {
                    Ok(output) => {
                        handle_command_output(app, &command_name, output, event_tx.clone()).await?;
                    }
                    Err(e) => {
                        app.add_message("system", &format!("Error: {}", e));
                    }
                }
            }
            Ok(true)
        }
        KeyCode::Esc => {
            app.hide_autocomplete();
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Show slash command help
async fn show_slash_command_help(app: &mut App) {
    let commands = app.command_registry.list().await;
    let mut help_text = String::from("Available slash commands:\n\n");
    for cmd in commands {
        help_text.push_str(&format!("  /{} - {}\n", cmd.name, cmd.description));
    }
    help_text.push_str("\nType /help for more information.");
    app.add_message("system", &help_text);
}

/// Execute a slash command
async fn execute_slash_command(
    app: &mut App,
    parsed: &ParsedCommand,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let ctx = create_command_context(app);
    let registry = app.command_registry.clone();
    match registry.execute(&parsed.name, &parsed.args, &ctx).await {
        Ok(output) => {
            handle_command_output(app, &parsed.name, output, event_tx.clone()).await?;
        }
        Err(e) => {
            app.add_message("system", &format!("Error: {}", e));
        }
    }
    Ok(())
}

/// Start streaming response from LLM
fn start_llm_stream(app: &mut App, input: &str, event_tx: &mpsc::Sender<AppEvent>) {
    app.add_message("user", input);
    app.is_processing = true;
    app.status = "Processing".to_string();
    app.add_message("assistant", "");

    let tx = event_tx.clone();
    let provider_id = app.provider_id.clone();
    let model_id = app.model_id.clone();
    let prompt = input.to_string();

    tokio::spawn(async move {
        if let Err(e) = stream_response_agentic(provider_id, model_id, prompt, tx.clone()).await {
            let _ = tx.send(AppEvent::StreamError(e.to_string())).await;
        }
    });
}

/// Handle submit action (Enter key)
async fn handle_submit(app: &mut App, event_tx: &mpsc::Sender<AppEvent>) -> Result<()> {
    if !app.is_ready() {
        app.open_model_selector();
        return Ok(());
    }

    if let Some(input) = app.take_input() {
        // Add to input history (before processing)
        app.add_input_to_history(&input);

        if input.trim() == "/" {
            show_slash_command_help(app).await;
        } else if let Some(parsed) = ParsedCommand::parse(&input) {
            execute_slash_command(app, &parsed, event_tx).await?;
        } else {
            start_llm_stream(app, &input, event_tx);
        }
    }
    Ok(())
}

/// Handle keyboard shortcuts (Ctrl+M, Ctrl+P)
fn handle_keyboard_shortcuts(app: &mut App, key: &KeyEvent) -> bool {
    if key.modifiers != KeyModifiers::CONTROL {
        return false;
    }

    match key.code {
        KeyCode::Char('m') => {
            app.open_model_selector();
            true
        }
        KeyCode::Char('p') => {
            app.open_provider_selector();
            true
        }
        _ => false,
    }
}

/// Handle keyboard input in the main loop
async fn handle_key_input(
    app: &mut App,
    key: KeyEvent,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Handle autocomplete first
    if handle_autocomplete_input(app, key, event_tx).await? {
        return Ok(());
    }

    // Handle dialog input if dialog is open
    if app.dialog.is_some() {
        handle_dialog_input(app, key, event_tx.clone()).await?;
        return Ok(());
    }

    // Check for keyboard shortcuts
    if handle_keyboard_shortcuts(app, &key) {
        return Ok(());
    }

    let action = key_to_action(key);

    match action {
        Action::Submit if !app.is_processing => {
            handle_submit(app, event_tx).await?;
        }
        Action::Cancel if app.is_processing => {
            app.is_processing = false;
            app.status = "Ready".to_string();
        }
        _ => {
            // Reset input history navigation when user starts typing
            if matches!(
                action,
                Action::Char(_) | Action::Backspace | Action::Delete | Action::ClearInput
            ) {
                app.input_history_position = None;
                app.input_history_buffer.clear();
            }

            app.handle_action(action);
            app.update_autocomplete().await;
        }
    }

    Ok(())
}

/// Handle a single async event
async fn handle_single_event(app: &mut App, event: AppEvent) -> Result<()> {
    match event {
        AppEvent::StreamDelta(text) => {
            app.append_to_assistant(&text);
        }
        AppEvent::StreamDone => {
            app.is_processing = false;
            app.status = "Ready".to_string();
            app.clear_tool_batch();
        }
        AppEvent::StreamError(err) => {
            app.is_processing = false;
            app.status = "Error".to_string();
            app.clear_tool_batch();
            app.add_message("system", &format!("Error: {}", err));
        }
        AppEvent::ToolCall(name, id) => {
            app.handle_tool_call(&id, &name);
        }
        AppEvent::DeviceCodeReceived {
            user_code,
            verification_uri,
            device_code,
            interval: _,
        } => {
            app.show_device_code(&user_code, &verification_uri, &device_code);
            let _ = open::that(&verification_uri);
        }
        AppEvent::OAuthSuccess { provider_id } => {
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
            app.handle_tool_result_grouped(&id, &output, is_error);
        }
        AppEvent::PermissionRequested(request) => {
            app.show_permission_request(request);
        }
        AppEvent::PermissionResponse { id, allow, scope } => {
            handle_permission_response(app, &id, allow, scope);
        }
        AppEvent::QuestionRequested(request) => {
            app.open_question_dialog(request);
        }
        AppEvent::QuestionReplied { id, answers } => {
            handle_question_reply(app, &id, answers);
        }
    }
    Ok(())
}

/// Handle permission response event
fn handle_permission_response(
    app: &mut App,
    id: &str,
    allow: bool,
    scope: crate::tool::PermissionScope,
) {
    use crate::tool::PermissionScope;

    // Send response to waiting tool
    let id_clone = id.to_string();
    tokio::spawn(async move {
        super::llm_streaming::send_permission_response(id_clone, allow, scope).await;
    });

    let scope_text = match scope {
        PermissionScope::Once => "once",
        PermissionScope::Session => "session",
        PermissionScope::Workspace => "workspace",
        PermissionScope::Global => "global",
    };

    app.status = if allow {
        format!("Permission granted ({}): {}", scope_text, id)
    } else {
        format!("Permission denied: {}", id)
    };
}

/// Handle question reply event
fn handle_question_reply(app: &mut App, id: &str, answers: Vec<Vec<String>>) {
    // Send response to waiting tool
    let id_clone = id.to_string();
    let answers_clone = answers.clone();
    tokio::spawn(async move {
        super::llm_streaming::send_question_response(id_clone, answers_clone).await;
    });

    // Format answers for display
    let formatted_answers: Vec<String> = answers
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| a.join(", "))
        .collect();

    app.status = if formatted_answers.is_empty() {
        "Question cancelled".to_string()
    } else {
        format!("Question answered: {}", formatted_answers.join(" | "))
    };
}

/// Process all pending async events
async fn handle_async_events(app: &mut App, event_rx: &mut mpsc::Receiver<AppEvent>) -> Result<()> {
    while let Ok(event) = event_rx.try_recv() {
        handle_single_event(app, event).await?;
    }
    Ok(())
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
        terminal.draw(|f| ui::render(f, app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key_input(app, key, &event_tx).await?;
            }
        }

        handle_async_events(app, event_rx).await?;

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
