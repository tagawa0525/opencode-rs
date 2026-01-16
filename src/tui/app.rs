//! Main TUI application state and event loop.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use super::input::{key_to_action, Action};
use super::theme::Theme;
use super::ui;
use crate::config::Config;
use crate::provider::{self, Model, Provider, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::tool;

/// Display message in the UI
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
    pub time_created: i64,
}

/// Active dialog type
#[derive(Debug, Clone, PartialEq)]
pub enum DialogType {
    None,
    ModelSelector,
    ProviderSelector,
    ApiKeyInput,
}

/// Item for selection dialogs
#[derive(Debug, Clone)]
pub struct SelectItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub provider_id: Option<String>,
}

/// Dialog state for selection dialogs
#[derive(Debug, Clone)]
pub struct DialogState {
    pub dialog_type: DialogType,
    pub items: Vec<SelectItem>,
    pub selected_index: usize,
    pub search_query: String,
    pub filtered_indices: Vec<usize>,
    pub input_value: String,
    pub title: String,
    pub message: Option<String>,
}

/// Application state
pub struct App {
    /// Current input text
    pub input: String,
    /// Cursor position in input
    pub cursor_position: usize,
    /// Display messages
    pub messages: Vec<DisplayMessage>,
    /// Current session
    pub session: Option<Session>,
    /// Session title
    pub session_title: String,
    /// Session slug
    pub session_slug: String,
    /// Model display string
    pub model_display: String,
    /// Provider ID
    pub provider_id: String,
    /// Model ID
    pub model_id: String,
    /// Current status
    pub status: String,
    /// Is currently processing
    pub is_processing: bool,
    /// Spinner animation frame
    pub spinner_frame: usize,
    /// Total cost
    pub total_cost: f64,
    /// Total tokens used
    pub total_tokens: u64,
    /// Theme
    pub theme: Theme,
    /// Should quit
    pub should_quit: bool,
    /// Whether model is configured
    pub model_configured: bool,
    /// Current dialog state
    pub dialog: Option<DialogState>,
    /// Available providers cache
    pub available_providers: Vec<Provider>,
    /// All providers cache
    pub all_providers: Vec<Provider>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            messages: Vec::new(),
            session: None,
            session_title: "New Session".to_string(),
            session_slug: String::new(),
            model_display: "No model selected".to_string(),
            provider_id: String::new(),
            model_id: String::new(),
            status: "Ready".to_string(),
            is_processing: false,
            spinner_frame: 0,
            total_cost: 0.0,
            total_tokens: 0,
            theme: Theme::dark(),
            should_quit: false,
            model_configured: false,
            dialog: None,
            available_providers: Vec::new(),
            all_providers: Vec::new(),
        }
    }
}

impl DialogState {
    pub fn new(dialog_type: DialogType, title: &str) -> Self {
        Self {
            dialog_type,
            items: Vec::new(),
            selected_index: 0,
            search_query: String::new(),
            filtered_indices: Vec::new(),
            input_value: String::new(),
            title: title.to_string(),
            message: None,
        }
    }

    pub fn with_items(mut self, items: Vec<SelectItem>) -> Self {
        self.filtered_indices = (0..items.len()).collect();
        self.items = items;
        self
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    pub fn update_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_indices = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.label.to_lowercase().contains(&query)
                        || item.id.to_lowercase().contains(&query)
                        || item
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&query))
                            .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected_index = 0;
    }

    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&i| self.items.get(i))
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected_index + 1 < self.filtered_indices.len() {
            self.selected_index += 1;
        }
    }
}

impl App {
    /// Create new app with model
    pub async fn new(model: Option<String>) -> Result<Self> {
        let config = Config::load().await?;
        let mut app = App::default();

        // Initialize provider registry
        provider::registry().initialize(&config).await?;

        // Cache providers
        app.all_providers = provider::registry().list().await;
        app.available_providers = provider::registry().list_available().await;

        // Try to get model
        let model_result = if let Some(m) = model {
            provider::parse_model_string(&m)
        } else {
            provider::registry().default_model(&config).await
        };

        if let Some((provider_id, model_id)) = model_result {
            app.provider_id = provider_id.clone();
            app.model_id = model_id.clone();
            app.model_display = format!("{}/{}", provider_id, model_id);
            app.model_configured = true;
        } else {
            // No model configured - will show dialog
            app.model_display = "No model configured".to_string();
            app.model_configured = false;
        }

        // Create session
        let session = Session::create(CreateSessionOptions::default()).await?;
        app.session_title = session.title.clone();
        app.session_slug = session.slug.clone();
        app.session = Some(session);

        // Apply theme from config
        if let Some(theme_name) = &config.theme {
            app.theme = match theme_name.as_str() {
                "light" => Theme::light(),
                _ => Theme::dark(),
            };
        }

        Ok(app)
    }

    /// Check if a model is configured and ready to use
    pub fn is_ready(&self) -> bool {
        self.model_configured && !self.provider_id.is_empty() && !self.model_id.is_empty()
    }

    /// Open the model selector dialog
    pub fn open_model_selector(&mut self) {
        let mut items = Vec::new();

        // Only show models from available providers (with API keys)
        for provider in &self.available_providers {
            for (model_id, model) in &provider.models {
                items.push(SelectItem {
                    id: format!("{}/{}", provider.id, model_id),
                    label: model.name.clone(),
                    description: Some(format!("{} - {}", provider.name, model_id)),
                    provider_id: Some(provider.id.clone()),
                });
            }
        }

        if items.is_empty() {
            // No available providers - open provider selector instead
            self.open_provider_selector();
            return;
        }

        let dialog = DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);
        self.dialog = Some(dialog);
    }

    /// Open the provider selector dialog
    pub fn open_provider_selector(&mut self) {
        let items: Vec<SelectItem> = self
            .all_providers
            .iter()
            .map(|p| {
                let has_key = p.key.is_some();
                SelectItem {
                    id: p.id.clone(),
                    label: p.name.clone(),
                    description: Some(if has_key {
                        "Connected".to_string()
                    } else {
                        format!("Set {}", p.env.first().unwrap_or(&"API_KEY".to_string()))
                    }),
                    provider_id: None,
                }
            })
            .collect();

        let dialog = DialogState::new(DialogType::ProviderSelector, "Connect Provider")
            .with_items(items)
            .with_message("Select a provider to configure");
        self.dialog = Some(dialog);
    }

    /// Open API key input dialog for a provider
    pub fn open_api_key_input(&mut self, provider_id: &str) {
        let provider = self.all_providers.iter().find(|p| p.id == provider_id);
        let env_var = provider
            .and_then(|p| p.env.first())
            .cloned()
            .unwrap_or_else(|| "API_KEY".to_string());

        let mut dialog = DialogState::new(DialogType::ApiKeyInput, "Enter API Key");
        dialog.message = Some(format!("Enter API key for {} ({})", provider_id, env_var));
        dialog.input_value = String::new();
        // Store provider_id in the first item
        dialog.items = vec![SelectItem {
            id: provider_id.to_string(),
            label: env_var,
            description: None,
            provider_id: Some(provider_id.to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Close the current dialog
    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }

    /// Set the current model
    pub async fn set_model(&mut self, provider_id: &str, model_id: &str) -> Result<()> {
        // Verify the model exists
        let model = provider::registry()
            .get_model(provider_id, model_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}/{}", provider_id, model_id))?;

        self.provider_id = provider_id.to_string();
        self.model_id = model_id.to_string();
        self.model_display = format!("{}/{}", provider_id, model.name);
        self.model_configured = true;
        self.close_dialog();

        Ok(())
    }

    /// Handle input action
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.should_quit = true;
            }
            Action::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += c.len_utf8();
            }
            Action::Backspace => {
                if self.cursor_position > 0 {
                    let prev_char_boundary = self.input[..self.cursor_position]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.remove(prev_char_boundary);
                    self.cursor_position = prev_char_boundary;
                }
            }
            Action::Delete => {
                if self.cursor_position < self.input.len() {
                    self.input.remove(self.cursor_position);
                }
            }
            Action::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position = self.input[..self.cursor_position]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            Action::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position = self.input[self.cursor_position..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor_position + i)
                        .unwrap_or(self.input.len());
                }
            }
            Action::Home => {
                self.cursor_position = 0;
            }
            Action::End => {
                self.cursor_position = self.input.len();
            }
            Action::Newline => {
                self.input.insert(self.cursor_position, '\n');
                self.cursor_position += 1;
            }
            Action::ClearInput => {
                self.input.clear();
                self.cursor_position = 0;
            }
            _ => {}
        }
    }

    /// Submit the current input
    pub fn take_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            return None;
        }
        let input = std::mem::take(&mut self.input);
        self.cursor_position = 0;
        Some(input)
    }

    /// Add a message to display
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(DisplayMessage {
            role: role.to_string(),
            content: content.to_string(),
            time_created: chrono::Utc::now().timestamp_millis(),
        });
    }

    /// Update the last assistant message
    pub fn update_last_assistant(&mut self, content: &str) {
        if let Some(msg) = self.messages.last_mut() {
            if msg.role == "assistant" {
                msg.content = content.to_string();
            }
        }
    }

    /// Append to the last assistant message
    pub fn append_to_assistant(&mut self, delta: &str) {
        if let Some(msg) = self.messages.last_mut() {
            if msg.role == "assistant" {
                msg.content.push_str(delta);
            }
        }
    }
}

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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
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
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

/// Application events
#[derive(Debug)]
enum AppEvent {
    StreamDelta(String),
    StreamDone,
    StreamError(String),
    ToolCall(String, String),
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
            if let Event::Key(key) = event::read()? {
                // Handle dialog input if dialog is open
                if app.dialog.is_some() {
                    handle_dialog_input(app, key).await?;
                } else {
                    let action = key_to_action(key);

                    // Check for model selector keybind (Ctrl+M)
                    if key.code == KeyCode::Char('m') && key.modifiers == KeyModifiers::CONTROL {
                        app.open_model_selector();
                        continue;
                    }

                    // Check for provider selector keybind (Ctrl+P)
                    if key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL {
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
                            // Add user message
                            app.add_message("user", &input);
                            app.is_processing = true;
                            app.status = "Processing".to_string();

                            // Add empty assistant message
                            app.add_message("assistant", "");

                            // Start streaming
                            let tx = event_tx.clone();
                            let provider_id = app.provider_id.clone();
                            let model_id = app.model_id.clone();
                            let prompt = input.clone();

                            tokio::spawn(async move {
                                match stream_response(&provider_id, &model_id, &prompt).await {
                                    Ok(mut rx) => {
                                        while let Some(event) = rx.recv().await {
                                            match event {
                                                StreamEvent::TextDelta(text) => {
                                                    let _ =
                                                        tx.send(AppEvent::StreamDelta(text)).await;
                                                }
                                                StreamEvent::Done { .. } => {
                                                    let _ = tx.send(AppEvent::StreamDone).await;
                                                }
                                                StreamEvent::Error(err) => {
                                                    let _ =
                                                        tx.send(AppEvent::StreamError(err)).await;
                                                }
                                                StreamEvent::ToolCallStart { name, .. } => {
                                                    let _ = tx
                                                        .send(AppEvent::ToolCall(
                                                            name,
                                                            String::new(),
                                                        ))
                                                        .await;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(AppEvent::StreamError(e.to_string())).await;
                                    }
                                }
                            });
                        }
                    } else if action == Action::Cancel && app.is_processing {
                        // Cancel processing
                        app.is_processing = false;
                        app.status = "Ready".to_string();
                    } else {
                        app.handle_action(action);
                    }
                }
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
                AppEvent::ToolCall(name, _args) => {
                    app.append_to_assistant(&format!("\n[Calling tool: {}]\n", name));
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
async fn handle_dialog_input(app: &mut App, key: crossterm::event::KeyEvent) -> Result<()> {
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
                                        // Need API key
                                        app.open_api_key_input(&provider_id);
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
        _ => {
            if key.code == KeyCode::Esc {
                app.close_dialog();
            }
        }
    }

    Ok(())
}

/// Stream a response from the LLM
async fn stream_response(
    provider_id: &str,
    model_id: &str,
    prompt: &str,
) -> Result<mpsc::Receiver<StreamEvent>> {
    let provider = provider::registry()
        .get(provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let model = provider
        .models
        .get(model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

    let api_key = provider
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    let messages = vec![provider::ChatMessage {
        role: "user".to_string(),
        content: provider::ChatContent::Text(prompt.to_string()),
    }];

    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<provider::ToolDefinition> = tools
        .into_iter()
        .map(|t| provider::ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    let client = provider::StreamingClient::new();

    match provider_id {
        "anthropic" => {
            client
                .stream_anthropic(
                    &api_key,
                    &model.api.id,
                    messages,
                    None,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        "openai" => {
            let base_url = model
                .api
                .url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1");
            client
                .stream_openai(
                    &api_key,
                    base_url,
                    &model.api.id,
                    messages,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    &api_key,
                    &model.api.id,
                    messages,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider_id)),
    }
}
