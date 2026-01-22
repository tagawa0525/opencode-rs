//! Type definitions for the TUI application.

use fuzzy_matcher::FuzzyMatcher;

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
    SessionRename,
    SessionList,
    Timeline,
    AgentSelector,
    Question,
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

/// Permission request from tool execution
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always: Vec<String>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

// Re-export question types from tool module to avoid duplication
pub use crate::tool::QuestionRequest;

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
    /// Selected permission option index (0=Once, 1=Session, 2=Workspace, 3=Global, 4=Reject)
    pub selected_permission_option: usize,
    /// For question requests
    pub question_request: Option<QuestionRequest>,
    /// Answers for each question (array of arrays)
    pub question_answers: Vec<Vec<String>>,
    /// Current question index (for multi-question mode)
    pub current_question_index: usize,
    /// Current option index (for navigation)
    pub current_option_index: usize,
    /// Custom answer input text
    pub custom_answer_input: String,
    /// Whether we're editing custom answer
    pub is_editing_custom: bool,
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
            selected_permission_option: 0,
            question_request: None,
            question_answers: Vec::new(),
            current_question_index: 0,
            current_option_index: 0,
            custom_answer_input: String::new(),
            is_editing_custom: false,
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

    pub fn with_question_request(mut self, request: QuestionRequest) -> Self {
        // Initialize answers array with empty vectors for each question
        self.question_answers = vec![Vec::new(); request.questions.len()];
        self.question_request = Some(request);
        self.current_question_index = 0;
        self.current_option_index = 0;
        self
    }

    pub fn update_filter(&mut self) {
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

    /// Move left in permission options
    pub fn move_permission_left(&mut self) {
        if self.selected_permission_option > 0 {
            self.selected_permission_option -= 1;
        }
    }

    /// Move right in permission options
    pub fn move_permission_right(&mut self) {
        // 0=Once, 1=Session, 2=Workspace, 3=Global, 4=Reject
        if self.selected_permission_option < 4 {
            self.selected_permission_option += 1;
        }
    }
}

/// Application events for the TUI event loop
#[derive(Debug)]
pub enum AppEvent {
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
        scope: crate::tool::PermissionScope,
    },
    QuestionRequested(QuestionRequest),
    QuestionReplied {
        id: String,
        answers: Vec<Vec<String>>,
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
}
