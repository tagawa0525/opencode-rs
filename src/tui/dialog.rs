//! Dialog handling for the TUI.
//!
//! This module contains dialog-related methods for the App and
//! the dialog input handler function.
//! Similar to ui/dialog.tsx in the TS version.

use anyhow::Result;
use crossterm::event::KeyCode;
use tokio::sync::mpsc;

use super::oauth_flow::{start_copilot_oauth_flow, start_openai_oauth_flow};
use super::state::App;
use super::types::{AppEvent, DialogState, DialogType, SelectItem};
use crate::config::Config;
use crate::provider;

/// Dialog-related methods for App
impl App {
    /// Open the model selector dialog
    pub fn open_model_selector(&mut self) {
        let items = self.collect_available_models();

        if items.is_empty() {
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

    /// Collect available models from providers, excluding deprecated ones
    fn collect_available_models(&self) -> Vec<SelectItem> {
        self.available_providers
            .iter()
            .flat_map(|provider| {
                provider
                    .models
                    .iter()
                    .filter(|(_, model)| {
                        !matches!(model.status, crate::provider::ModelStatus::Deprecated)
                    })
                    .map(move |(model_id, model)| SelectItem {
                        id: format!("{}/{}", provider.id, model_id),
                        label: format!("{}{}", model.name, model_status_badge(model.status)),
                        description: Some(format!("{} - {}", provider.name, model_id)),
                        provider_id: Some(provider.id.clone()),
                    })
            })
            .collect()
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

    /// Open provider connection dialog (alias for open_provider_selector)
    pub fn open_provider_connection(&mut self) {
        self.open_provider_selector();
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

    /// Open session rename dialog
    pub fn open_session_rename(&mut self) {
        let current_title = self.session_title.clone();
        let mut dialog = DialogState::new(DialogType::SessionRename, "Rename Session");
        dialog.message = Some("Enter a new name for this session".to_string());
        dialog.input_value = current_title;
        self.dialog = Some(dialog);
    }

    /// Open session list dialog
    pub async fn open_session_list(&mut self) -> Result<()> {
        use crate::session::Session;

        let sessions = Session::list("default").await?;

        let items: Vec<SelectItem> = sessions
            .into_iter()
            .map(|s| {
                let created_time = chrono::DateTime::from_timestamp_millis(s.time.created)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());

                SelectItem {
                    id: s.id.clone(),
                    label: s.title.clone(),
                    description: Some(format!("Created: {} | Slug: {}", created_time, s.slug)),
                    provider_id: None,
                }
            })
            .collect();

        let dialog = DialogState::new(DialogType::SessionList, "Select Session")
            .with_items(items)
            .with_message("Select a session to switch to");
        self.dialog = Some(dialog);

        Ok(())
    }

    /// Open agent selector dialog
    pub async fn open_agent_selector(&mut self) -> Result<()> {
        // Load config to get agent definitions
        let config = Config::load().await?;

        let items: Vec<SelectItem> = if let Some(agents) = config.agent {
            agents
                .into_iter()
                .filter(|(_, agent_config)| !agent_config.disable.unwrap_or(false))
                .filter(|(_, agent_config)| !agent_config.hidden.unwrap_or(false))
                .map(|(name, agent_config)| SelectItem {
                    id: name.clone(),
                    label: name.clone(),
                    description: agent_config.description.or(agent_config.model),
                    provider_id: None,
                })
                .collect()
        } else {
            vec![]
        };

        if items.is_empty() {
            self.add_message(
                "system",
                "No agents configured. Define agents in your opencode.json config file.",
            );
            return Ok(());
        }

        let dialog = DialogState::new(DialogType::AgentSelector, "Select Agent")
            .with_items(items)
            .with_message("Select an agent to use");
        self.dialog = Some(dialog);

        Ok(())
    }

    /// Open timeline dialog (message history)
    pub fn open_timeline(&mut self) {
        let items: Vec<SelectItem> = self
            .messages
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let role_display = match msg.role.as_str() {
                    "user" => "👤 User",
                    "assistant" => "🤖 Assistant",
                    "system" => "⚙️  System",
                    _ => &msg.role,
                };

                // Get first line of content for preview
                let preview = msg
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect::<String>();

                let preview = if msg.content.len() > 60 {
                    format!("{}...", preview)
                } else {
                    preview
                };

                SelectItem {
                    id: idx.to_string(),
                    label: format!("{}: {}", role_display, preview),
                    description: Some(format!("Message {}/{}", idx + 1, self.messages.len())),
                    provider_id: None,
                }
            })
            .collect();

        let dialog = DialogState::new(DialogType::Timeline, "Message Timeline")
            .with_items(items)
            .with_message("Select a message to view");
        self.dialog = Some(dialog);
    }

    /// Open auth method selector for a provider
    pub fn open_auth_method_selector(&mut self, provider_id: &str) {
        let items = match get_auth_method_items(provider_id) {
            Some(items) => items,
            None => {
                // For other providers, go directly to API key input
                self.open_api_key_input(provider_id);
                return;
            }
        };

        let provider_name = self.get_provider_name(provider_id);
        let dialog = DialogState::new(DialogType::AuthMethodSelector, "Select Auth Method")
            .with_items(items)
            .with_message(&format!("How do you want to connect to {}?", provider_name));
        self.dialog = Some(dialog);
    }

    /// Get provider name by ID
    fn get_provider_name(&self, provider_id: &str) -> String {
        self.all_providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.to_string())
    }

    /// Start OAuth flow for a provider
    fn start_oauth_dialog(&mut self, provider_id: &str, title: &str, message: &str) {
        let mut dialog = DialogState::new(DialogType::OAuthWaiting, title);
        dialog.message = Some(message.to_string());
        dialog.items = vec![SelectItem {
            id: provider_id.to_string(),
            label: provider_id.to_string(),
            description: None,
            provider_id: Some(provider_id.to_string()),
        }];
        self.dialog = Some(dialog);
    }

    /// Start GitHub Copilot OAuth device flow
    pub fn start_copilot_oauth(&mut self) {
        self.start_oauth_dialog(
            "copilot",
            "GitHub Copilot Sign In",
            "Requesting device code...",
        );
    }

    /// Start OpenAI OAuth PKCE flow
    pub fn start_openai_oauth(&mut self) {
        self.start_oauth_dialog(
            "openai",
            "ChatGPT Sign In",
            "Opening browser for authentication...",
        );
    }

    /// Update dialog with device code info
    pub fn show_device_code(&mut self, user_code: &str, verification_uri: &str) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        dialog.dialog_type = DialogType::OAuthDeviceCode;
        dialog.user_code = Some(user_code.to_string());
        dialog.verification_uri = Some(verification_uri.to_string());
        dialog.message = Some(format!(
            "Go to: {}\n\nEnter code: {}",
            verification_uri, user_code
        ));
    }

    /// Open question dialog
    pub fn open_question_dialog(&mut self, request: super::types::QuestionRequest) {
        let dialog =
            DialogState::new(DialogType::Question, "Question").with_question_request(request);
        self.dialog = Some(dialog);
    }
}

// ============================================================================
// Dialog Input Handlers
// ============================================================================

/// Handle input for model/provider selector dialogs
async fn handle_selector_input(app: &mut App, key_code: KeyCode) -> Result<()> {
    let Some(dialog) = &mut app.dialog else {
        return Ok(());
    };

    match key_code {
        KeyCode::Esc => {
            if !app.model_configured && dialog.dialog_type == DialogType::ModelSelector {
                app.should_quit = true;
            }
            app.close_dialog();
        }
        KeyCode::Enter => {
            handle_selector_enter(app).await?;
        }
        KeyCode::Up => dialog.move_up(),
        KeyCode::Down => dialog.move_down(),
        KeyCode::Char(c) => {
            dialog.search_query.push(c);
            dialog.update_filter();
        }
        KeyCode::Backspace => {
            dialog.search_query.pop();
            dialog.update_filter();
        }
        _ => {}
    }
    Ok(())
}

/// Handle Enter key in selector dialogs
async fn handle_selector_enter(app: &mut App) -> Result<()> {
    let Some(dialog) = &app.dialog else {
        return Ok(());
    };
    let Some(item) = dialog.selected_item() else {
        return Ok(());
    };
    let item_id = item.id.clone();
    let dialog_type = dialog.dialog_type.clone();

    match dialog_type {
        DialogType::ModelSelector => {
            if let Some((provider_id, model_id)) = provider::parse_model_string(&item_id) {
                app.set_model(&provider_id, &model_id).await?;
            }
        }
        DialogType::ProviderSelector => {
            let has_key = app
                .all_providers
                .iter()
                .any(|p| p.id == item_id && p.key.is_some());

            if has_key {
                app.close_dialog();
                app.open_model_selector();
            } else {
                app.open_auth_method_selector(&item_id);
            }
        }
        DialogType::SessionList => {
            use crate::session::Session;
            if let Ok(Some(session)) = Session::get("default", &item_id).await {
                app.session_title = session.title.clone();
                app.session_slug = session.slug.clone();
                let session_title = session.title.clone();
                app.session = Some(session);
                app.messages.clear();
                app.total_cost = 0.0;
                app.total_tokens = 0;
                app.add_message("system", &format!("Switched to session: {}", session_title));
            }
            app.close_dialog();
        }
        DialogType::AgentSelector => {
            app.status = format!(
                "Agent switching to '{}' - agent system under development",
                item_id
            );
            app.add_message(
                "system",
                &format!(
                    "Note: Agent '{}' selected, but agent system is not yet fully implemented",
                    item_id
                ),
            );
            app.close_dialog();
        }
        DialogType::Timeline => {
            if let Ok(msg_index) = item_id.parse::<usize>() {
                if let Some(msg) = app.messages.get(msg_index) {
                    app.add_message(
                        "system",
                        &format!(
                            "Message {}/{} ({})\n\n{}",
                            msg_index + 1,
                            app.messages.len(),
                            msg.role,
                            msg.content
                        ),
                    );
                }
            }
            app.close_dialog();
        }
        _ => {}
    }
    Ok(())
}

/// Handle character input for text dialogs
fn handle_text_char(dialog: &mut DialogState, key_code: KeyCode) {
    match key_code {
        KeyCode::Char(c) => dialog.input_value.push(c),
        KeyCode::Backspace => {
            dialog.input_value.pop();
        }
        _ => {}
    }
}

/// Handle input for API key input dialog
async fn handle_api_key_input(app: &mut App, key_code: KeyCode) -> Result<()> {
    match key_code {
        KeyCode::Esc => app.open_provider_selector(),
        KeyCode::Enter => handle_api_key_submit(app).await?,
        _ => {
            if let Some(dialog) = &mut app.dialog {
                handle_text_char(dialog, key_code);
            }
        }
    }
    Ok(())
}

/// Handle input for session rename dialog
async fn handle_rename_input(app: &mut App, key_code: KeyCode) -> Result<()> {
    match key_code {
        KeyCode::Esc => app.close_dialog(),
        KeyCode::Enter => handle_rename_submit(app).await?,
        _ => {
            if let Some(dialog) = &mut app.dialog {
                handle_text_char(dialog, key_code);
            }
        }
    }
    Ok(())
}

/// Handle session rename submission
async fn handle_rename_submit(app: &mut App) -> Result<()> {
    let new_title = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return Ok(()),
        };
        dialog.input_value.trim().to_string()
    };

    if new_title.is_empty() {
        return Ok(());
    }

    // Update session
    if let Some(session) = &mut app.session {
        session.title = new_title.clone();
        let project_id = session.project_id.clone();
        session
            .update(&project_id, |s| {
                s.title = new_title.clone();
            })
            .await?;
        app.session_title = new_title;
        app.add_message("system", "Session renamed successfully");
    }

    app.close_dialog();
    Ok(())
}

/// Handle API key submission
async fn handle_api_key_submit(app: &mut App) -> Result<()> {
    let (api_key, provider_id, env_var) = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return Ok(()),
        };
        let first_item = match dialog.items.first() {
            Some(i) => i,
            None => return Ok(()),
        };
        (
            dialog.input_value.clone(),
            first_item.id.clone(),
            first_item.label.clone(),
        )
    };

    if api_key.is_empty() {
        return Ok(());
    }

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
    if let Err(e) = crate::auth::save_api_key(&provider_id, &api_key).await {
        eprintln!("Warning: Failed to save API key: {}", e);
    }

    Ok(())
}

/// Handle input for auth method selector dialog
async fn handle_auth_method_input(
    app: &mut App,
    key_code: KeyCode,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let Some(dialog) = &mut app.dialog else {
        return Ok(());
    };

    match key_code {
        KeyCode::Esc => app.open_provider_selector(),
        KeyCode::Up => dialog.move_up(),
        KeyCode::Down => dialog.move_down(),
        KeyCode::Enter => {
            let Some(item) = dialog.selected_item() else {
                return Ok(());
            };
            let auth_method = item.id.clone();
            let provider_id = item.provider_id.clone().unwrap_or_default();

            match auth_method.as_str() {
                "oauth" => start_oauth_flow(app, &provider_id, event_tx),
                "api_key" => app.open_api_key_input(&provider_id),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Start OAuth flow for a provider
fn start_oauth_flow(app: &mut App, provider_id: &str, event_tx: &mpsc::Sender<AppEvent>) {
    match provider_id {
        "copilot" => {
            app.start_copilot_oauth();
            let tx = event_tx.clone();
            tokio::spawn(async move {
                start_copilot_oauth_flow(tx).await;
            });
        }
        "openai" => {
            app.start_openai_oauth();
            let tx = event_tx.clone();
            tokio::spawn(async move {
                start_openai_oauth_flow(tx).await;
            });
        }
        _ => {}
    }
}

/// Map permission option index to (allow, scope)
fn permission_option_to_response(index: usize) -> Option<(bool, crate::tool::PermissionScope)> {
    use crate::tool::PermissionScope;
    match index {
        0 => Some((true, PermissionScope::Once)),
        1 => Some((true, PermissionScope::Session)),
        2 => Some((true, PermissionScope::Workspace)),
        3 => Some((true, PermissionScope::Global)),
        4 => Some((false, PermissionScope::Once)),
        _ => None,
    }
}

/// Handle input for permission request dialog
async fn handle_permission_input(
    app: &mut App,
    key_code: KeyCode,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    use crate::tool::PermissionScope;

    let Some(dialog) = &mut app.dialog else {
        return Ok(());
    };
    let Some(permission_request) = &dialog.permission_request else {
        return Ok(());
    };
    let id = permission_request.id.clone();

    // Arrow key navigation
    match key_code {
        KeyCode::Left => {
            dialog.move_permission_left();
            return Ok(());
        }
        KeyCode::Right => {
            dialog.move_permission_right();
            return Ok(());
        }
        _ => {}
    }

    // Get response based on key press or current selection
    let response = match key_code {
        KeyCode::Char('y' | 'Y') => Some((true, PermissionScope::Once)),
        KeyCode::Char('s' | 'S') => Some((true, PermissionScope::Session)),
        KeyCode::Char('w' | 'W') => Some((true, PermissionScope::Workspace)),
        KeyCode::Char('g' | 'G') => Some((true, PermissionScope::Global)),
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Some((false, PermissionScope::Once)),
        KeyCode::Enter => permission_option_to_response(dialog.selected_permission_option),
        _ => None,
    };

    if let Some((allow, scope)) = response {
        app.close_dialog();
        let _ = event_tx
            .send(AppEvent::PermissionResponse { id, allow, scope })
            .await;
    }
    Ok(())
}

/// Handle custom answer editing in question dialog
fn handle_custom_answer_edit(dialog: &mut DialogState, key_code: KeyCode) {
    let current_q_idx = dialog.current_question_index;
    match key_code {
        KeyCode::Esc => {
            dialog.is_editing_custom = false;
            dialog.custom_answer_input.clear();
        }
        KeyCode::Enter => {
            let custom_answer = dialog.custom_answer_input.trim().to_string();
            if !custom_answer.is_empty() {
                dialog.question_answers[current_q_idx].push(custom_answer);
            }
            dialog.is_editing_custom = false;
            dialog.custom_answer_input.clear();
        }
        KeyCode::Char(c) => dialog.custom_answer_input.push(c),
        KeyCode::Backspace => {
            dialog.custom_answer_input.pop();
        }
        _ => {}
    }
}

/// Handle input for question dialog
async fn handle_question_input(
    app: &mut App,
    key_code: KeyCode,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let Some(dialog) = &mut app.dialog else {
        return Ok(());
    };
    let Some(question_request) = dialog.question_request.clone() else {
        return Ok(());
    };

    // Handle custom answer editing mode separately
    if dialog.is_editing_custom {
        handle_custom_answer_edit(dialog, key_code);
        return Ok(());
    }

    let question_count = question_request.questions.len();
    let current_q_idx = dialog.current_question_index;
    let current_question = &question_request.questions[current_q_idx];
    let max_options = current_question.options.len() + usize::from(current_question.custom);

    match key_code {
        KeyCode::Esc => app.close_dialog(),
        KeyCode::Up | KeyCode::Char('k') => {
            dialog.current_option_index = dialog.current_option_index.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if dialog.current_option_index + 1 < max_options {
                dialog.current_option_index += 1;
            }
        }
        KeyCode::Tab if current_q_idx + 1 < question_count => {
            dialog.current_question_index += 1;
            dialog.current_option_index = 0;
        }
        KeyCode::BackTab if current_q_idx > 0 => {
            dialog.current_question_index -= 1;
            dialog.current_option_index = 0;
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(digit) = c.to_digit(10) {
                let option_idx = (digit as usize).saturating_sub(1);
                if option_idx < current_question.options.len() {
                    toggle_answer(dialog, current_q_idx, option_idx, current_question.multiple);
                }
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let opt_idx = dialog.current_option_index;
            if current_question.custom && opt_idx == current_question.options.len() {
                dialog.is_editing_custom = true;
                dialog.custom_answer_input.clear();
            } else if opt_idx < current_question.options.len() {
                toggle_answer(dialog, current_q_idx, opt_idx, current_question.multiple);
                // Single-select: auto-advance or auto-submit
                if !current_question.multiple {
                    if question_count == 1 {
                        submit_answers(app, event_tx, &question_request).await?;
                    } else if current_q_idx + 1 < question_count {
                        dialog.current_question_index += 1;
                        dialog.current_option_index = 0;
                    }
                }
            }
        }
        KeyCode::Char('c') if current_question.custom => {
            dialog.is_editing_custom = true;
            dialog.custom_answer_input.clear();
        }
        KeyCode::Char('s') => {
            submit_answers(app, event_tx, &question_request).await?;
        }
        _ => {}
    }

    Ok(())
}

/// Toggle an answer selection
fn toggle_answer(dialog: &mut DialogState, question_idx: usize, option_idx: usize, multiple: bool) {
    let question_request = match &dialog.question_request {
        Some(req) => req,
        None => return,
    };

    let current_question = &question_request.questions[question_idx];
    let option_label = current_question.options[option_idx].label.clone();

    let answers = &mut dialog.question_answers[question_idx];

    if multiple {
        // Multi-select: toggle
        if let Some(pos) = answers.iter().position(|a| a == &option_label) {
            answers.remove(pos);
        } else {
            answers.push(option_label);
        }
    } else {
        // Single-select: replace
        *answers = vec![option_label];
    }
}

/// Submit all answers
async fn submit_answers(
    app: &mut App,
    event_tx: &mpsc::Sender<AppEvent>,
    question_request: &super::types::QuestionRequest,
) -> Result<()> {
    let (id, answers) = {
        let dialog = match &app.dialog {
            Some(d) => d,
            None => return Ok(()),
        };
        (question_request.id.clone(), dialog.question_answers.clone())
    };

    app.close_dialog();
    let _ = event_tx
        .send(AppEvent::QuestionReplied { id, answers })
        .await;
    Ok(())
}

/// Handle input when a dialog is open
pub async fn handle_dialog_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let Some(dialog) = &app.dialog else {
        return Ok(());
    };

    match &dialog.dialog_type {
        DialogType::ModelSelector
        | DialogType::ProviderSelector
        | DialogType::SessionList
        | DialogType::Timeline
        | DialogType::AgentSelector => {
            handle_selector_input(app, key.code).await?;
        }
        DialogType::ApiKeyInput => {
            handle_api_key_input(app, key.code).await?;
        }
        DialogType::SessionRename => {
            handle_rename_input(app, key.code).await?;
        }
        DialogType::AuthMethodSelector => {
            handle_auth_method_input(app, key.code, event_tx).await?;
        }
        DialogType::OAuthDeviceCode | DialogType::OAuthWaiting => {
            if key.code == KeyCode::Esc {
                app.open_provider_selector();
            }
        }
        DialogType::PermissionRequest => {
            handle_permission_input(app, key.code, event_tx).await?;
        }
        DialogType::Question => {
            handle_question_input(app, key.code, event_tx).await?;
        }
    }

    Ok(())
}

/// Get status badge text for a model status
fn model_status_badge(status: crate::provider::ModelStatus) -> &'static str {
    match status {
        crate::provider::ModelStatus::Alpha => " [ALPHA]",
        crate::provider::ModelStatus::Beta => " [BETA]",
        crate::provider::ModelStatus::Active => "",
        crate::provider::ModelStatus::Deprecated => " [DEPRECATED]",
    }
}

/// Get auth method items for a provider, if OAuth is supported
fn get_auth_method_items(provider_id: &str) -> Option<Vec<SelectItem>> {
    let (oauth_label, oauth_desc, key_label, key_desc) = match provider_id {
        "copilot" => (
            "Sign in with GitHub",
            "Use your GitHub Copilot subscription",
            "Enter token manually",
            "Enter GITHUB_COPILOT_TOKEN directly",
        ),
        "openai" => (
            "Sign in with ChatGPT",
            "Use your ChatGPT Plus/Pro subscription",
            "Enter API key",
            "Enter OPENAI_API_KEY directly",
        ),
        _ => return None,
    };

    Some(vec![
        SelectItem {
            id: "oauth".to_string(),
            label: oauth_label.to_string(),
            description: Some(oauth_desc.to_string()),
            provider_id: Some(provider_id.to_string()),
        },
        SelectItem {
            id: "api_key".to_string(),
            label: key_label.to_string(),
            description: Some(key_desc.to_string()),
            provider_id: Some(provider_id.to_string()),
        },
    ])
}
