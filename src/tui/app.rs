//! Main TUI application state and event loop.

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
use super::theme::Theme;
use super::ui;

// Re-export types for backward compatibility
pub use super::types::{
    AppEvent, AutocompleteState, CommandItem, DialogState, DialogType, DisplayMessage, MessagePart,
    PermissionRequest, SelectItem,
};
use crate::config::Config;
use crate::provider::{self, Model, Provider, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::slash_command::{
    builtin::*, parser::ParsedCommand, registry::CommandRegistry, template::TemplateCommand,
    CommandAction, CommandContext, CommandOutput,
};
use crate::tool;

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
    /// Slash command registry
    pub command_registry: std::sync::Arc<CommandRegistry>,
    /// Autocomplete state
    pub autocomplete: Option<AutocompleteState>,
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
            command_registry: std::sync::Arc::new(CommandRegistry::new()),
            autocomplete: None,
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

        // Create session first
        let session = Session::create(CreateSessionOptions::default()).await?;
        app.session_title = session.title.clone();
        app.session_slug = session.slug.clone();

        // Load model with priority: CLI arg > Session > Workspace config > Global config > Last used
        let model_result = if let Some(m) = model {
            // CLI argument takes highest priority
            provider::parse_model_string(&m)
        } else if let Some(session_model) = session.get_model().await {
            // Session model is second priority
            Some((session_model.provider_id, session_model.model_id))
        } else if let Some(configured_model) = config.model.as_ref() {
            // Workspace/global config is third priority
            provider::parse_model_string(configured_model)
        } else {
            // Fall back to last used model from global storage
            match crate::storage::global()
                .read::<String>(&["state", "last_model"])
                .await
            {
                Ok(Some(last_model)) => {
                    tracing::debug!("Loaded last used model: {}", last_model);
                    provider::parse_model_string(&last_model)
                }
                Ok(None) => {
                    tracing::debug!("No last used model found");
                    None
                }
                Err(e) => {
                    tracing::warn!("Failed to load last used model: {}", e);
                    None
                }
            }
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

        app.session = Some(session);

        // Apply theme from config
        if let Some(theme_name) = &config.theme {
            app.theme = match theme_name.as_str() {
                "light" => Theme::light(),
                _ => Theme::dark(),
            };
        }

        // Initialize slash commands
        app.init_commands(&config).await;

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
        // By default, hide deprecated models (users can show them with a toggle)
        for provider in &self.available_providers {
            for (model_id, model) in &provider.models {
                // Skip deprecated models by default
                if matches!(model.status, crate::provider::ModelStatus::Deprecated) {
                    continue;
                }

                // Add status indicator to the label
                let status_badge = match model.status {
                    crate::provider::ModelStatus::Alpha => " [ALPHA]",
                    crate::provider::ModelStatus::Beta => " [BETA]",
                    crate::provider::ModelStatus::Active => "",
                    crate::provider::ModelStatus::Deprecated => " [DEPRECATED]",
                };

                items.push(SelectItem {
                    id: format!("{}/{}", provider.id, model_id),
                    label: format!("{}{}", model.name, status_badge),
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

        let dialog = DialogState::new(
            DialogType::ModelSelector,
            "Select Model (deprecated models hidden)",
        )
        .with_items(items);
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

    /// Open auth method selector for a provider
    pub fn open_auth_method_selector(&mut self, provider_id: &str) {
        let mut items = Vec::new();

        match provider_id {
            "copilot" => {
                items.push(SelectItem {
                    id: "oauth".to_string(),
                    label: "Sign in with GitHub".to_string(),
                    description: Some("Use your GitHub Copilot subscription".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
                items.push(SelectItem {
                    id: "api_key".to_string(),
                    label: "Enter token manually".to_string(),
                    description: Some("Enter GITHUB_COPILOT_TOKEN directly".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
            }
            "openai" => {
                items.push(SelectItem {
                    id: "oauth".to_string(),
                    label: "Sign in with ChatGPT".to_string(),
                    description: Some("Use your ChatGPT Plus/Pro subscription".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
                items.push(SelectItem {
                    id: "api_key".to_string(),
                    label: "Enter API key".to_string(),
                    description: Some("Enter OPENAI_API_KEY directly".to_string()),
                    provider_id: Some(provider_id.to_string()),
                });
            }
            _ => {
                // For other providers, go directly to API key input
                self.open_api_key_input(provider_id);
                return;
            }
        }

        let provider_name = self
            .all_providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.to_string());

        let dialog = DialogState::new(DialogType::AuthMethodSelector, "Select Auth Method")
            .with_items(items)
            .with_message(&format!("How do you want to connect to {}?", provider_name));
        self.dialog = Some(dialog);
    }

    /// Start GitHub Copilot OAuth device flow
    pub fn start_copilot_oauth(&mut self) {
        let mut dialog = DialogState::new(DialogType::OAuthWaiting, "GitHub Copilot Sign In");
        dialog.message = Some("Requesting device code...".to_string());
        dialog.items = vec![SelectItem {
            id: "copilot".to_string(),
            label: "copilot".to_string(),
            description: None,
            provider_id: Some("copilot".to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Update dialog with device code info
    pub fn show_device_code(&mut self, user_code: &str, verification_uri: &str, device_code: &str) {
        if let Some(dialog) = &mut self.dialog {
            dialog.dialog_type = DialogType::OAuthDeviceCode;
            dialog.user_code = Some(user_code.to_string());
            dialog.verification_uri = Some(verification_uri.to_string());
            dialog.device_code = Some(device_code.to_string());
            dialog.message = Some(format!(
                "Go to: {}\n\nEnter code: {}",
                verification_uri, user_code
            ));
        }
    }

    /// Start OpenAI OAuth PKCE flow
    pub fn start_openai_oauth(&mut self) {
        let mut dialog = DialogState::new(DialogType::OAuthWaiting, "ChatGPT Sign In");
        dialog.message = Some("Opening browser for authentication...".to_string());
        dialog.items = vec![SelectItem {
            id: "openai".to_string(),
            label: "openai".to_string(),
            description: None,
            provider_id: Some("openai".to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Close the current dialog
    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }

    /// Show permission request dialog
    pub fn show_permission_request(&mut self, request: PermissionRequest) {
        let mut dialog = DialogState::new(DialogType::PermissionRequest, "Permission Request");
        dialog.permission_request = Some(request);
        self.dialog = Some(dialog);
    }

    /// Show autocomplete for slash commands
    pub async fn show_autocomplete(&mut self, filter: &str) {
        use fuzzy_matcher::FuzzyMatcher;

        let commands = self.command_registry.list().await;
        let mut items: Vec<CommandItem> = commands
            .into_iter()
            .map(|cmd| CommandItem {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
                display: format!("/{}", cmd.name),
            })
            .collect();

        // Apply fuzzy filtering if there's a filter
        if !filter.is_empty() {
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            let mut scored_items: Vec<(i64, CommandItem)> = items
                .into_iter()
                .filter_map(|item| {
                    let score = matcher.fuzzy_match(&item.name, filter)?;
                    Some((score, item))
                })
                .collect();

            // Sort by score (descending)
            scored_items.sort_by(|a, b| b.0.cmp(&a.0));
            items = scored_items.into_iter().map(|(_, item)| item).collect();
        }

        // Limit to 10 items
        items.truncate(10);

        if !items.is_empty() {
            let mut state = AutocompleteState::new(items);
            state.filter = filter.to_string();
            self.autocomplete = Some(state);
        } else {
            self.autocomplete = None;
        }
    }

    /// Hide autocomplete
    pub fn hide_autocomplete(&mut self) {
        self.autocomplete = None;
    }

    /// Update autocomplete based on current input
    pub async fn update_autocomplete(&mut self) {
        // Check if input starts with "/" and cursor is at a position where autocomplete makes sense
        if self.input.starts_with('/') {
            // Find the filter text (everything after / until cursor or first space)
            let cursor_pos = self.cursor_position.min(self.input.len());
            let input_until_cursor = self.input[..cursor_pos].to_string();

            // If there's a space before cursor, hide autocomplete
            if input_until_cursor.contains(' ') {
                self.hide_autocomplete();
                return;
            }

            // Extract filter (text after /)
            let filter = input_until_cursor[1..].to_string(); // Remove leading /
            self.show_autocomplete(&filter).await;
        } else {
            self.hide_autocomplete();
        }
    }

    /// Insert selected autocomplete item and return the command name
    pub fn insert_autocomplete_selection(&mut self) -> Option<String> {
        if let Some(autocomplete) = &self.autocomplete {
            if let Some(item) = autocomplete.selected_item() {
                let command_name = item.name.clone();
                self.hide_autocomplete();
                // Clear the input - we'll execute the command directly
                self.input.clear();
                self.cursor_position = 0;
                return Some(command_name);
            }
        }
        None
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

        // Save to session
        if let Some(session) = &mut self.session {
            let model_ref = crate::session::ModelRef {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            };
            if let Err(e) = session
                .set_model(&session.project_id.clone(), model_ref)
                .await
            {
                tracing::warn!("Failed to save model to session: {}", e);
            }
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
            parts: vec![MessagePart::Text {
                text: content.to_string(),
            }],
        });
    }

    /// Add a tool call to the last message
    pub fn add_tool_call(&mut self, id: &str, name: &str, args: &str) {
        if let Some(msg) = self.messages.last_mut() {
            msg.parts.push(MessagePart::ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                args: args.to_string(),
            });
        }
    }

    /// Add a tool result to the messages
    pub fn add_tool_result(&mut self, id: &str, output: &str, is_error: bool) {
        if let Some(msg) = self.messages.last_mut() {
            msg.parts.push(MessagePart::ToolResult {
                id: id.to_string(),
                output: output.to_string(),
                is_error,
            });
        }
    }

    /// Update the last assistant message
    pub fn update_last_assistant(&mut self, content: &str) {
        if let Some(msg) = self.messages.last_mut() {
            if msg.role == "assistant" {
                msg.content = content.to_string();
            }
        }
    }

    /// Copy text to clipboard using both OSC 52 and system clipboard
    fn copy_to_clipboard(&self, text: &str) -> Result<()> {
        // Use OSC 52 for terminal clipboard integration
        self.copy_via_osc52(text)?;

        // Also try system clipboard
        use arboard::Clipboard;
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(text);
        }

        Ok(())
    }

    /// Copy text to clipboard using OSC 52 escape sequence
    fn copy_via_osc52(&self, text: &str) -> Result<()> {
        use base64::Engine;
        let base64_text = base64::engine::general_purpose::STANDARD.encode(text);
        let osc52 = format!("\x1b]52;c;{}\x07", base64_text);

        // Check if running in tmux
        let osc52_final = if std::env::var("TMUX").is_ok() {
            // Wrap OSC 52 for tmux
            format!("\x1bPtmux;\x1b{}\x1b\\", osc52)
        } else {
            osc52
        };

        // Write to stdout
        use std::io::Write;
        let mut stdout = std::io::stdout();
        stdout.write_all(osc52_final.as_bytes())?;
        stdout.flush()?;

        Ok(())
    }

    /// Initialize slash commands
    async fn init_commands(&mut self, config: &Config) {
        // Register built-in commands
        self.command_registry
            .register(std::sync::Arc::new(HelpCommand))
            .await;
        self.command_registry
            .register(std::sync::Arc::new(ClearCommand))
            .await;
        self.command_registry
            .register(std::sync::Arc::new(ModelCommand))
            .await;
        self.command_registry
            .register(std::sync::Arc::new(AgentCommand))
            .await;

        // Register custom commands from config
        if let Some(commands) = &config.command {
            for (name, cmd_config) in commands {
                let template_cmd = TemplateCommand::new(name.clone(), cmd_config.clone());
                self.command_registry
                    .register(std::sync::Arc::new(template_cmd))
                    .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    mod autocomplete_state {
        use super::*;

        fn create_items() -> Vec<CommandItem> {
            vec![
                CommandItem {
                    name: "help".to_string(),
                    description: "Show help".to_string(),
                    display: "/help".to_string(),
                },
                CommandItem {
                    name: "model".to_string(),
                    description: "Select model".to_string(),
                    display: "/model".to_string(),
                },
                CommandItem {
                    name: "clear".to_string(),
                    description: "Clear session".to_string(),
                    display: "/clear".to_string(),
                },
            ]
        }

        #[test]
        fn test_new() {
            let items = create_items();
            let state = AutocompleteState::new(items.clone());

            assert_eq!(state.items.len(), 3);
            assert_eq!(state.selected_index, 0);
            assert_eq!(state.filter, "");
        }

        #[test]
        fn test_move_down() {
            let items = create_items();
            let mut state = AutocompleteState::new(items);

            assert_eq!(state.selected_index, 0);
            state.move_down();
            assert_eq!(state.selected_index, 1);
            state.move_down();
            assert_eq!(state.selected_index, 2);
            // Should wrap around
            state.move_down();
            assert_eq!(state.selected_index, 0);
        }

        #[test]
        fn test_move_up() {
            let items = create_items();
            let mut state = AutocompleteState::new(items);

            assert_eq!(state.selected_index, 0);
            // Should wrap to end
            state.move_up();
            assert_eq!(state.selected_index, 2);
            state.move_up();
            assert_eq!(state.selected_index, 1);
        }

        #[test]
        fn test_selected_item() {
            let items = create_items();
            let state = AutocompleteState::new(items);

            let selected = state.selected_item().unwrap();
            assert_eq!(selected.name, "help");
        }

        #[test]
        fn test_empty_items() {
            let state = AutocompleteState::new(vec![]);
            assert!(state.selected_item().is_none());
        }
    }

    mod dialog_state {
        use super::*;

        fn create_items() -> Vec<SelectItem> {
            vec![
                SelectItem {
                    id: "anthropic/claude-3-5-sonnet".to_string(),
                    label: "Claude 3.5 Sonnet".to_string(),
                    description: Some("Anthropic's latest".to_string()),
                    provider_id: Some("anthropic".to_string()),
                },
                SelectItem {
                    id: "openai/gpt-4o".to_string(),
                    label: "GPT-4o".to_string(),
                    description: Some("OpenAI's flagship".to_string()),
                    provider_id: Some("openai".to_string()),
                },
                SelectItem {
                    id: "anthropic/claude-3-opus".to_string(),
                    label: "Claude 3 Opus".to_string(),
                    description: Some("Most powerful".to_string()),
                    provider_id: Some("anthropic".to_string()),
                },
            ]
        }

        #[test]
        fn test_new() {
            let dialog = DialogState::new(DialogType::ModelSelector, "Select Model");

            assert_eq!(dialog.dialog_type, DialogType::ModelSelector);
            assert_eq!(dialog.title, "Select Model");
            assert_eq!(dialog.selected_index, 0);
            assert!(dialog.items.is_empty());
        }

        #[test]
        fn test_with_items() {
            let items = create_items();
            let dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            assert_eq!(dialog.items.len(), 3);
            assert_eq!(dialog.filtered_indices.len(), 3);
        }

        #[test]
        fn test_move_down() {
            let items = create_items();
            let mut dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            assert_eq!(dialog.selected_index, 0);
            dialog.move_down();
            assert_eq!(dialog.selected_index, 1);
            dialog.move_down();
            assert_eq!(dialog.selected_index, 2);
            // Does not wrap
            dialog.move_down();
            assert_eq!(dialog.selected_index, 2);
        }

        #[test]
        fn test_move_up() {
            let items = create_items();
            let mut dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            dialog.selected_index = 2;
            dialog.move_up();
            assert_eq!(dialog.selected_index, 1);
            dialog.move_up();
            assert_eq!(dialog.selected_index, 0);
            // Does not wrap
            dialog.move_up();
            assert_eq!(dialog.selected_index, 0);
        }

        #[test]
        fn test_selected_item() {
            let items = create_items();
            let dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            let selected = dialog.selected_item().unwrap();
            assert_eq!(selected.id, "anthropic/claude-3-5-sonnet");
        }

        #[test]
        fn test_update_filter_empty() {
            let items = create_items();
            let mut dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            dialog.search_query = "".to_string();
            dialog.update_filter();

            assert_eq!(dialog.filtered_indices.len(), 3);
        }

        #[test]
        fn test_update_filter_matches() {
            let items = create_items();
            let mut dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            dialog.search_query = "claude".to_string();
            dialog.update_filter();

            // Should match Claude models
            assert!(dialog.filtered_indices.len() >= 2);
        }

        #[test]
        fn test_update_filter_no_match() {
            let items = create_items();
            let mut dialog =
                DialogState::new(DialogType::ModelSelector, "Select Model").with_items(items);

            dialog.search_query = "xyz123notfound".to_string();
            dialog.update_filter();

            assert!(dialog.filtered_indices.is_empty());
        }
    }

    mod app_action_handling {
        use super::*;

        #[test]
        fn test_char_action() {
            let mut app = App::default();
            app.handle_action(Action::Char('a'));

            assert_eq!(app.input, "a");
            assert_eq!(app.cursor_position, 1);
        }

        #[test]
        fn test_multiple_chars() {
            let mut app = App::default();
            app.handle_action(Action::Char('H'));
            app.handle_action(Action::Char('i'));

            assert_eq!(app.input, "Hi");
            assert_eq!(app.cursor_position, 2);
        }

        #[test]
        fn test_backspace() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 5;

            app.handle_action(Action::Backspace);

            assert_eq!(app.input, "Hell");
            assert_eq!(app.cursor_position, 4);
        }

        #[test]
        fn test_backspace_at_start() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 0;

            app.handle_action(Action::Backspace);

            assert_eq!(app.input, "Hello");
            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_delete() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 0;

            app.handle_action(Action::Delete);

            assert_eq!(app.input, "ello");
            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_cursor_left() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 3;

            app.handle_action(Action::Left);

            assert_eq!(app.cursor_position, 2);
        }

        #[test]
        fn test_cursor_right() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 2;

            app.handle_action(Action::Right);

            assert_eq!(app.cursor_position, 3);
        }

        #[test]
        fn test_home() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 3;

            app.handle_action(Action::Home);

            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_end() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 0;

            app.handle_action(Action::End);

            assert_eq!(app.cursor_position, 5);
        }

        #[test]
        fn test_newline() {
            let mut app = App::default();
            app.input = "Hello".to_string();
            app.cursor_position = 5;

            app.handle_action(Action::Newline);

            assert_eq!(app.input, "Hello\n");
            assert_eq!(app.cursor_position, 6);
        }

        #[test]
        fn test_clear_input() {
            let mut app = App::default();
            app.input = "Hello World".to_string();
            app.cursor_position = 6;

            app.handle_action(Action::ClearInput);

            assert_eq!(app.input, "");
            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_quit() {
            let mut app = App::default();
            assert!(!app.should_quit);

            app.handle_action(Action::Quit);

            assert!(app.should_quit);
        }
    }

    mod app_message_handling {
        use super::*;

        #[test]
        fn test_add_message() {
            let mut app = App::default();
            app.add_message("user", "Hello");

            assert_eq!(app.messages.len(), 1);
            assert_eq!(app.messages[0].role, "user");
            assert_eq!(app.messages[0].content, "Hello");
        }

        #[test]
        fn test_add_tool_call() {
            let mut app = App::default();
            app.add_message("assistant", "Let me help");
            app.add_tool_call("call_123", "bash", r#"{"cmd": "ls"}"#);

            assert_eq!(app.messages.len(), 1);
            assert_eq!(app.messages[0].parts.len(), 2);
        }

        #[test]
        fn test_update_last_assistant() {
            let mut app = App::default();
            app.add_message("assistant", "Processing...");
            app.update_last_assistant("Done!");

            assert_eq!(app.messages[0].content, "Done!");
        }

        #[test]
        fn test_append_to_assistant() {
            let mut app = App::default();
            app.add_message("assistant", "Hello");
            app.append_to_assistant(" World");

            assert_eq!(app.messages[0].content, "Hello World");
        }

        #[test]
        fn test_take_input() {
            let mut app = App::default();
            app.input = "Hello World".to_string();
            app.cursor_position = 11;

            let input = app.take_input();

            assert_eq!(input, Some("Hello World".to_string()));
            assert_eq!(app.input, "");
            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_take_input_empty() {
            let mut app = App::default();
            app.input = "   ".to_string();

            let input = app.take_input();

            assert!(input.is_none());
        }
    }

    mod app_state {
        use super::*;

        #[test]
        fn test_default() {
            let app = App::default();

            assert_eq!(app.input, "");
            assert_eq!(app.cursor_position, 0);
            assert!(app.messages.is_empty());
            assert!(app.session.is_none());
            assert!(!app.is_processing);
            assert!(!app.should_quit);
            assert!(!app.model_configured);
        }

        #[test]
        fn test_is_ready_not_configured() {
            let app = App::default();
            assert!(!app.is_ready());
        }

        #[test]
        fn test_is_ready_configured() {
            let mut app = App::default();
            app.model_configured = true;
            app.provider_id = "anthropic".to_string();
            app.model_id = "claude-3-5-sonnet".to_string();

            assert!(app.is_ready());
        }

        #[test]
        fn test_close_dialog() {
            let mut app = App::default();
            app.dialog = Some(DialogState::new(DialogType::ModelSelector, "Test"));

            app.close_dialog();

            assert!(app.dialog.is_none());
        }

        #[test]
        fn test_hide_autocomplete() {
            let mut app = App::default();
            app.autocomplete = Some(AutocompleteState::new(vec![]));

            app.hide_autocomplete();

            assert!(app.autocomplete.is_none());
        }
    }

    mod unicode_handling {
        use super::*;

        #[test]
        fn test_unicode_char() {
            let mut app = App::default();
            app.handle_action(Action::Char('日'));
            app.handle_action(Action::Char('本'));
            app.handle_action(Action::Char('語'));

            assert_eq!(app.input, "日本語");
            assert_eq!(app.cursor_position, 9); // 3 bytes per char
        }

        #[test]
        fn test_unicode_backspace() {
            let mut app = App::default();
            app.input = "日本語".to_string();
            app.cursor_position = 9; // End of string

            app.handle_action(Action::Backspace);

            assert_eq!(app.input, "日本");
            assert_eq!(app.cursor_position, 6);
        }

        #[test]
        fn test_unicode_cursor_movement() {
            let mut app = App::default();
            app.input = "日本語".to_string();
            app.cursor_position = 9; // End

            app.handle_action(Action::Left);
            assert_eq!(app.cursor_position, 6); // Before 語

            app.handle_action(Action::Left);
            assert_eq!(app.cursor_position, 3); // Before 本

            app.handle_action(Action::Right);
            assert_eq!(app.cursor_position, 6); // After 本
        }

        #[test]
        fn test_emoji_handling() {
            let mut app = App::default();
            app.input = "Hello 👋".to_string();
            app.cursor_position = app.input.len();

            // Should handle emoji properly
            app.handle_action(Action::Backspace);
            assert_eq!(app.input, "Hello ");
        }
    }
}
