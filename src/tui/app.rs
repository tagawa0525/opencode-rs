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
use crate::slash_command::{
    builtin::*, parser::ParsedCommand, registry::CommandRegistry, template::TemplateCommand,
    CommandAction, CommandContext, CommandOutput,
};
use crate::tool;

/// Display message in the UI
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
    pub time_created: i64,
    pub parts: Vec<MessagePart>,
}

/// Message part - can be text, tool call, or tool result
#[derive(Debug, Clone)]
pub enum MessagePart {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        args: String,
    },
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
}

/// Active dialog type
#[derive(Debug, Clone, PartialEq)]
pub enum DialogType {
    None,
    ModelSelector,
    ProviderSelector,
    ApiKeyInput,
    AuthMethodSelector,
    OAuthDeviceCode,
    OAuthWaiting,
    PermissionRequest,
}

/// Autocomplete state for slash commands
#[derive(Debug, Clone)]
pub struct AutocompleteState {
    /// Available commands to choose from
    pub items: Vec<CommandItem>,
    /// Currently selected index
    pub selected_index: usize,
    /// The filter text (after the /)
    pub filter: String,
}

/// Item in autocomplete list
#[derive(Debug, Clone)]
pub struct CommandItem {
    pub name: String,
    pub description: String,
    pub display: String,
}

impl AutocompleteState {
    pub fn new(items: Vec<CommandItem>) -> Self {
        Self {
            items,
            selected_index: 0,
            filter: String::new(),
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = self.items.len().saturating_sub(1);
        }
    }

    pub fn move_down(&mut self) {
        if self.selected_index + 1 < self.items.len() {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
    }

    pub fn selected_item(&self) -> Option<&CommandItem> {
        self.items.get(self.selected_index)
    }
}

/// Item for selection dialogs
#[derive(Debug, Clone)]
pub struct SelectItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub provider_id: Option<String>,
}

/// Permission request for tool execution
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub description: String,
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
    /// For OAuth device code flow
    pub device_code: Option<String>,
    pub user_code: Option<String>,
    pub verification_uri: Option<String>,
    /// For permission requests
    pub permission_request: Option<PermissionRequest>,
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
            device_code: None,
            user_code: None,
            verification_uri: None,
            permission_request: None,
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
        use fuzzy_matcher::FuzzyMatcher;

        if self.search_query.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();

            // Score each item and filter
            let mut scored_items: Vec<(usize, i64)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(idx, item)| {
                    // Try matching against label, id, and description
                    let label_score = matcher.fuzzy_match(&item.label, &self.search_query);
                    let id_score = matcher.fuzzy_match(&item.id, &self.search_query);
                    let desc_score = item
                        .description
                        .as_ref()
                        .and_then(|d| matcher.fuzzy_match(d, &self.search_query));

                    // Use the best score
                    let best_score = [label_score, id_score, desc_score]
                        .into_iter()
                        .flatten()
                        .max()?;

                    Some((idx, best_score))
                })
                .collect();

            // Sort by score (descending)
            scored_items.sort_by(|a, b| b.1.cmp(&a.1));

            self.filtered_indices = scored_items.into_iter().map(|(idx, _)| idx).collect();
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
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
    PermissionRequested(PermissionRequest),
    PermissionResponse {
        id: String,
        allow: bool,
        always: bool,
    },
    // OAuth events
    DeviceCodeReceived {
        user_code: String,
        verification_uri: String,
        device_code: String,
        interval: u64,
    },
    OAuthSuccess {
        provider_id: String,
    },
    OAuthError(String),
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
                            // Check if input is just "/" - show help for slash commands
                            if input.trim() == "/" {
                                // Show available slash commands
                                let commands = app.command_registry.list().await;
                                let mut help_text = String::from("Available slash commands:\n\n");
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

/// Stream a response from the LLM with agentic loop
async fn stream_response_agentic(
    provider_id: String,
    model_id: String,
    initial_prompt: String,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    use crate::config::Config;
    use crate::permission::PermissionChecker;
    use crate::provider::{self, ChatContent, ChatMessage, ContentPart, StreamingClient};
    use crate::tool::{self, DoomLoopDetector, ToolCallTracker};
    use std::sync::Arc;
    use tokio::sync::oneshot;

    let config = Config::load().await?;
    let permission_checker = PermissionChecker::from_config(&config);

    let provider = provider::registry()
        .get(&provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let model = provider
        .models
        .get(&model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

    let api_key = provider
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    // Prepare tool definitions
    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<provider::ToolDefinition> = tools
        .into_iter()
        .map(|t| provider::ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    // Tool execution context
    let tool_ctx = Arc::new(tool::ToolContext {
        session_id: String::new(),
        message_id: String::new(),
        agent: "tui".to_string(),
        abort: None,
        cwd: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| ".".to_string()),
        root: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| ".".to_string()),
        extra: Default::default(),
    });

    // Initialize conversation with user message
    let mut messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(initial_prompt),
    }];

    let client = StreamingClient::new();
    let mut step = 0;
    let max_steps = 10;
    let mut doom_detector = DoomLoopDetector::new();

    // Create channel for permission responses (for future use)
    let (_perm_tx, mut _perm_rx) = mpsc::channel::<(String, bool, bool)>(10);

    loop {
        step += 1;
        if step > max_steps {
            let _ = event_tx
                .send(AppEvent::StreamError(
                    "Maximum agentic loop steps reached".to_string(),
                ))
                .await;
            break;
        }

        // Stream the response
        let mut rx = match provider_id.as_str() {
            "anthropic" => {
                client
                    .stream_anthropic(
                        &api_key,
                        &model.api.id,
                        messages.clone(),
                        None,
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
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
                        messages.clone(),
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
            }
            "copilot" => {
                client
                    .stream_copilot(
                        &api_key,
                        &model.api.id,
                        messages.clone(),
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported provider: {}", provider_id));
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
                    let _ = event_tx.send(AppEvent::StreamDelta(text.clone())).await;
                    response_text.push_str(&text);
                }
                StreamEvent::ToolCallStart { id, name } => {
                    let _ = event_tx
                        .send(AppEvent::ToolCall(name.clone(), id.clone()))
                        .await;
                    tool_tracker.start_call(id, name);
                }
                StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    tool_tracker.add_arguments(&id, &arguments_delta);
                }
                StreamEvent::Done {
                    finish_reason: reason,
                } => {
                    finish_reason = reason;
                }
                StreamEvent::Error(err) => {
                    let _ = event_tx.send(AppEvent::StreamError(err.clone())).await;
                    return Err(anyhow::anyhow!(err));
                }
                _ => {}
            }
        }

        // Add assistant response to conversation
        if !response_text.is_empty() {
            assistant_parts.push(ContentPart::Text {
                text: response_text.clone(),
            });
        }

        // Check if there are tool calls to execute
        let pending_calls = tool_tracker.get_all_calls();

        if !pending_calls.is_empty() {
            // Check for doom loop
            doom_detector.add_calls(&pending_calls);

            if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
                // Request permission for doom loop continuation
                let req_id = format!("doom_loop_{}", step);
                let request = PermissionRequest {
                    id: req_id.clone(),
                    tool_name: "doom_loop".to_string(),
                    arguments: format!("Tool '{}' with args: {}", tool_name, args),
                    description: format!(
                        "Doom loop detected: '{}' called {} times with identical arguments",
                        tool_name,
                        tool::DOOM_LOOP_THRESHOLD
                    ),
                };

                let _ = event_tx.send(AppEvent::PermissionRequested(request)).await;

                // Wait for permission response
                // TODO: Implement async permission waiting
                // For now, break the loop
                break;
            }

            // Request permissions for each tool call
            // TODO: Implement async permission system
            // For now, auto-allow read/glob/grep, ask for others

            let mut approved_calls = Vec::new();
            for call in &pending_calls {
                let action = permission_checker.check_tool(&call.name);

                match action {
                    crate::config::PermissionAction::Allow => {
                        approved_calls.push(call.clone());
                    }
                    crate::config::PermissionAction::Deny => {
                        // Skip this call
                    }
                    crate::config::PermissionAction::Ask => {
                        // Request permission via dialog
                        let req_id = format!("tool_{}_{}", call.id, step);
                        let request = PermissionRequest {
                            id: req_id.clone(),
                            tool_name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            description: format!("Allow tool '{}' to execute?", call.name),
                        };

                        let _ = event_tx.send(AppEvent::PermissionRequested(request)).await;

                        // Wait for permission response
                        // TODO: Implement proper async waiting
                        // For now, just skip
                    }
                }
            }

            if approved_calls.is_empty() {
                break;
            }

            // Add tool use parts to assistant message
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

            // Execute tools in parallel
            let tool_results = tool::execute_all_tools_parallel(approved_calls, &tool_ctx).await;

            // Send tool results as events
            for result in &tool_results {
                if let ContentPart::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } = result
                {
                    let _ = event_tx
                        .send(AppEvent::ToolResult {
                            id: tool_use_id.clone(),
                            output: content.clone(),
                            is_error: is_error.unwrap_or(false),
                        })
                        .await;
                }
            }

            // Add tool results to conversation
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Parts(tool_results),
            });

            // Continue loop
        } else {
            // No tool calls - add final assistant message and exit loop
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: if assistant_parts.len() == 1 {
                    match &assistant_parts[0] {
                        ContentPart::Text { text } => ChatContent::Text(text.clone()),
                        _ => ChatContent::Parts(assistant_parts),
                    }
                } else if !assistant_parts.is_empty() {
                    ChatContent::Parts(assistant_parts)
                } else {
                    ChatContent::Text(String::new())
                },
            });

            // Check finish reason
            if finish_reason == "stop" || finish_reason == "end_turn" || finish_reason == "length" {
                break;
            }
        }
    }

    let _ = event_tx.send(AppEvent::StreamDone).await;
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

/// Start GitHub Copilot OAuth device flow
async fn start_copilot_oauth_flow(tx: mpsc::Sender<AppEvent>) {
    use crate::oauth;

    // Request device code
    match oauth::copilot_request_device_code().await {
        Ok(device_code_response) => {
            // Send device code to UI immediately
            let _ = tx
                .send(AppEvent::DeviceCodeReceived {
                    user_code: device_code_response.user_code.clone(),
                    verification_uri: device_code_response.verification_uri.clone(),
                    device_code: device_code_response.device_code.clone(),
                    interval: device_code_response.interval,
                })
                .await;

            // Start polling in a separate task (non-blocking)
            let device_code = device_code_response.device_code;
            let interval = device_code_response.interval;
            tokio::spawn(async move {
                poll_copilot_token(tx, device_code, interval).await;
            });
        }
        Err(e) => {
            let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
        }
    }
}

/// Poll for GitHub Copilot token in background
async fn poll_copilot_token(tx: mpsc::Sender<AppEvent>, device_code: String, interval: u64) {
    use crate::oauth;
    use std::time::Duration;
    use tokio::time::timeout;

    // Timeout after 15 minutes (device codes typically expire after 15 min)
    let poll_result = timeout(
        Duration::from_secs(900),
        oauth::copilot_poll_for_token(&device_code, interval),
    )
    .await;

    match poll_result {
        Ok(Ok(access_token)) => {
            // Save token
            let token_info = oauth::OAuthTokenInfo::new_copilot(access_token.clone());
            if let Err(e) = crate::auth::save_oauth_token("copilot", token_info).await {
                let _ = tx
                    .send(AppEvent::OAuthError(format!("Failed to save token: {}", e)))
                    .await;
                return;
            }

            // Also set environment variable for current session
            std::env::set_var("GITHUB_COPILOT_TOKEN", &access_token);

            let _ = tx
                .send(AppEvent::OAuthSuccess {
                    provider_id: "copilot".to_string(),
                })
                .await;
        }
        Ok(Err(e)) => {
            let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
        }
        Err(_) => {
            let _ = tx
                .send(AppEvent::OAuthError(
                    "Authentication timed out. Please try again.".to_string(),
                ))
                .await;
        }
    }
}

/// Start OpenAI OAuth PKCE flow
async fn start_openai_oauth_flow(tx: mpsc::Sender<AppEvent>) {
    use crate::oauth;

    // Generate PKCE codes and state
    let pkce = oauth::generate_pkce();
    let state = oauth::generate_state();
    let redirect_uri = oauth::get_oauth_redirect_uri();

    // Start callback server
    let callback_rx = match oauth::start_oauth_callback_server(state.clone()).await {
        Ok(rx) => rx,
        Err(e) => {
            let _ = tx
                .send(AppEvent::OAuthError(format!(
                    "Failed to start callback server: {}",
                    e
                )))
                .await;
            return;
        }
    };

    // Build and open auth URL
    let auth_url = oauth::build_openai_auth_url(&redirect_uri, &pkce, &state);
    if let Err(e) = open::that(&auth_url) {
        let _ = tx
            .send(AppEvent::OAuthError(format!(
                "Failed to open browser: {}",
                e
            )))
            .await;
        return;
    }

    // Handle callback in a separate task (non-blocking)
    tokio::spawn(async move {
        handle_openai_callback(tx, callback_rx, redirect_uri, pkce).await;
    });
}

/// Handle OpenAI OAuth callback in background
async fn handle_openai_callback(
    tx: mpsc::Sender<AppEvent>,
    callback_rx: tokio::sync::oneshot::Receiver<String>,
    redirect_uri: String,
    pkce: crate::oauth::PkceCodes,
) {
    use crate::oauth;
    use std::time::Duration;
    use tokio::time::timeout;

    // Timeout after 5 minutes for user to complete browser auth
    let callback_result = timeout(Duration::from_secs(300), callback_rx).await;

    match callback_result {
        Ok(Ok(code)) => {
            // Exchange code for tokens
            match oauth::openai_exchange_code(&code, &redirect_uri, &pkce).await {
                Ok(tokens) => {
                    // Save tokens
                    let token_info = oauth::OAuthTokenInfo::new_openai(tokens);
                    if let Err(e) =
                        crate::auth::save_oauth_token("openai", token_info.clone()).await
                    {
                        let _ = tx
                            .send(AppEvent::OAuthError(format!(
                                "Failed to save tokens: {}",
                                e
                            )))
                            .await;
                        return;
                    }

                    // Set environment variable for current session
                    std::env::set_var("OPENAI_API_KEY", &token_info.access);

                    let _ = tx
                        .send(AppEvent::OAuthSuccess {
                            provider_id: "openai".to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::OAuthError(e.to_string())).await;
                }
            }
        }
        Ok(Err(_)) => {
            let _ = tx
                .send(AppEvent::OAuthError("OAuth callback failed".to_string()))
                .await;
        }
        Err(_) => {
            let _ = tx
                .send(AppEvent::OAuthError(
                    "Authentication timed out. Please try again.".to_string(),
                ))
                .await;
        }
    }
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
