//! Main TUI application state and event loop.

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
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
use crate::provider::{self, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::tool;

/// Display message in the UI
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
    pub time_created: i64,
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
        }
    }
}

impl App {
    /// Create new app with model
    pub async fn new(model: Option<String>) -> Result<Self> {
        let config = Config::load().await?;
        let mut app = App::default();

        // Get model
        let (provider_id, model_id) = if let Some(m) = model {
            provider::parse_model_string(&m)
                .ok_or_else(|| anyhow::anyhow!("Invalid model format"))?
        } else {
            provider::registry()
                .default_model(&config)
                .await
                .ok_or_else(|| anyhow::anyhow!("No model configured"))?
        };

        app.provider_id = provider_id.clone();
        app.model_id = model_id.clone();
        app.model_display = format!("{}/{}", provider_id, model_id);

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
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(model).await?;

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
                let action = key_to_action(key);

                if action == Action::Submit && !app.is_processing {
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
                                                let _ = tx.send(AppEvent::StreamDelta(text)).await;
                                            }
                                            StreamEvent::Done { .. } => {
                                                let _ = tx.send(AppEvent::StreamDone).await;
                                            }
                                            StreamEvent::Error(err) => {
                                                let _ = tx.send(AppEvent::StreamError(err)).await;
                                            }
                                            StreamEvent::ToolCallStart { name, .. } => {
                                                let _ = tx
                                                    .send(AppEvent::ToolCall(name, String::new()))
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
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider_id)),
    }
}
