//! Application state management.
//!
//! This module contains the App struct and its associated methods for managing
//! the TUI application state. Similar to context/local.tsx in the TS version.

use anyhow::Result;
use std::sync::Arc;

use super::input::Action;
use super::theme::Theme;
use super::types::{
    AutocompleteState, CommandItem, DialogState, DialogType, DisplayMessage, MessagePart,
    PermissionRequest, SelectItem,
};
use crate::config::Config;
use crate::provider::{self, Provider};
use crate::session::{CreateSessionOptions, Session};
use crate::slash_command::{builtin::*, registry::CommandRegistry, template::TemplateCommand};

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
    pub command_registry: Arc<CommandRegistry>,
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
            command_registry: Arc::new(CommandRegistry::new()),
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

    /// Hide autocomplete
    pub fn hide_autocomplete(&mut self) {
        self.autocomplete = None;
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

    /// Append to the last assistant message
    pub fn append_to_assistant(&mut self, delta: &str) {
        if let Some(msg) = self.messages.last_mut() {
            if msg.role == "assistant" {
                msg.content.push_str(delta);
            }
        }
    }

    /// Copy text to clipboard using both OSC 52 and system clipboard
    pub fn copy_to_clipboard(&self, text: &str) -> Result<()> {
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
    pub async fn init_commands(&mut self, config: &Config) {
        // Register built-in commands
        self.command_registry.register(Arc::new(HelpCommand)).await;
        self.command_registry.register(Arc::new(ClearCommand)).await;
        self.command_registry.register(Arc::new(ModelCommand)).await;
        self.command_registry.register(Arc::new(AgentCommand)).await;

        // Register custom commands from config
        if let Some(commands) = &config.command {
            for (name, cmd_config) in commands {
                let template_cmd = TemplateCommand::new(name.clone(), cmd_config.clone());
                self.command_registry.register(Arc::new(template_cmd)).await;
            }
        }

        // Load commands from markdown files
        match crate::slash_command::loader::load_all_commands().await {
            Ok(commands) => {
                for cmd in commands {
                    tracing::info!("Registering command from markdown: {}", cmd.name());
                    self.command_registry.register(cmd).await;
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load commands from markdown files: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod app_state {
        use super::*;

        #[test]
        fn test_default() {
            let app = App::default();
            assert!(app.input.is_empty());
            assert_eq!(app.cursor_position, 0);
            assert!(app.messages.is_empty());
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

    mod app_actions {
        use super::*;

        #[test]
        fn test_handle_action_char() {
            let mut app = App::default();
            app.handle_action(Action::Char('a'));
            assert_eq!(app.input, "a");
            assert_eq!(app.cursor_position, 1);
        }

        #[test]
        fn test_handle_action_backspace() {
            let mut app = App::default();
            app.input = "ab".to_string();
            app.cursor_position = 2;
            app.handle_action(Action::Backspace);
            assert_eq!(app.input, "a");
            assert_eq!(app.cursor_position, 1);
        }

        #[test]
        fn test_handle_action_left_right() {
            let mut app = App::default();
            app.input = "ab".to_string();
            app.cursor_position = 2;
            app.handle_action(Action::Left);
            assert_eq!(app.cursor_position, 1);
            app.handle_action(Action::Right);
            assert_eq!(app.cursor_position, 2);
        }

        #[test]
        fn test_handle_action_home_end() {
            let mut app = App::default();
            app.input = "abc".to_string();
            app.cursor_position = 1;
            app.handle_action(Action::Home);
            assert_eq!(app.cursor_position, 0);
            app.handle_action(Action::End);
            assert_eq!(app.cursor_position, 3);
        }

        #[test]
        fn test_handle_action_quit() {
            let mut app = App::default();
            app.handle_action(Action::Quit);
            assert!(app.should_quit);
        }
    }

    mod app_message_handling {
        use super::*;

        #[test]
        fn test_take_input() {
            let mut app = App::default();
            app.input = "hello".to_string();
            app.cursor_position = 5;
            let input = app.take_input();
            assert_eq!(input, Some("hello".to_string()));
            assert!(app.input.is_empty());
            assert_eq!(app.cursor_position, 0);
        }

        #[test]
        fn test_take_input_empty() {
            let mut app = App::default();
            app.input = "   ".to_string();
            let input = app.take_input();
            assert!(input.is_none());
        }

        #[test]
        fn test_append_to_assistant() {
            let mut app = App::default();
            app.add_message("assistant", "Hello");
            app.append_to_assistant(" world");
            assert_eq!(app.messages[0].content, "Hello world");
        }

        #[test]
        fn test_update_last_assistant() {
            let mut app = App::default();
            app.add_message("assistant", "Hello");
            app.update_last_assistant("Goodbye");
            assert_eq!(app.messages[0].content, "Goodbye");
        }
    }

    mod unicode_handling {
        use super::*;

        #[test]
        fn test_unicode_char() {
            let mut app = App::default();
            app.handle_action(Action::Char('日'));
            assert_eq!(app.input, "日");
            assert_eq!(app.cursor_position, 3);
        }

        #[test]
        fn test_unicode_backspace() {
            let mut app = App::default();
            app.input = "日本".to_string();
            app.cursor_position = 6;
            app.handle_action(Action::Backspace);
            assert_eq!(app.input, "日");
            assert_eq!(app.cursor_position, 3);
        }

        #[test]
        fn test_unicode_cursor_movement() {
            let mut app = App::default();
            app.input = "日本語".to_string();
            app.cursor_position = 9;
            app.handle_action(Action::Left);
            assert_eq!(app.cursor_position, 6);
            app.handle_action(Action::Right);
            assert_eq!(app.cursor_position, 9);
        }

        #[test]
        fn test_emoji_handling() {
            let mut app = App::default();
            app.handle_action(Action::Char('🎉'));
            assert_eq!(app.input, "🎉");
            assert_eq!(app.cursor_position, 4);
        }
    }
}
