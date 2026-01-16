//! Input handling for the TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input action that can be triggered by key events
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Quit the application
    Quit,
    /// Submit the current input
    Submit,
    /// Cancel current operation
    Cancel,
    /// Insert a newline
    Newline,
    /// Move cursor up
    Up,
    /// Move cursor down
    Down,
    /// Move cursor left
    Left,
    /// Move cursor right
    Right,
    /// Move to start of line
    Home,
    /// Move to end of line
    End,
    /// Delete character before cursor
    Backspace,
    /// Delete character at cursor
    Delete,
    /// Insert character
    Char(char),
    /// Paste from clipboard
    Paste,
    /// Open model selector
    ModelSelector,
    /// Open session list
    SessionList,
    /// Create new session
    NewSession,
    /// Toggle sidebar
    ToggleSidebar,
    /// Scroll up
    ScrollUp,
    /// Scroll down
    ScrollDown,
    /// Page up
    PageUp,
    /// Page down
    PageDown,
    /// Go to top
    Top,
    /// Go to bottom
    Bottom,
    /// Clear input
    ClearInput,
    /// Undo
    Undo,
    /// Redo
    Redo,
    /// No action
    None,
}

/// Convert a key event to an action
pub fn key_to_action(key: KeyEvent) -> Action {
    match key {
        // Quit
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Action::Quit,

        // Submit (Enter)
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Submit,

        // Newline (Shift+Enter, Ctrl+Enter, Alt+Enter)
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::SHIFT,
            ..
        }
        | KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::CONTROL,
            ..
        }
        | KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::ALT,
            ..
        } => Action::Newline,

        // Cancel (Escape)
        KeyEvent {
            code: KeyCode::Esc, ..
        } => Action::Cancel,

        // Navigation
        KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Up,
        KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Down,
        KeyEvent {
            code: KeyCode::Left,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Left,
        KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Right,
        KeyEvent {
            code: KeyCode::Home,
            ..
        } => Action::Home,
        KeyEvent {
            code: KeyCode::End, ..
        } => Action::End,

        // Editing
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => Action::Backspace,
        KeyEvent {
            code: KeyCode::Delete,
            ..
        } => Action::Delete,

        // Line navigation shortcuts
        KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Action::Home,
        KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Action::End,

        // Clear input
        KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Action::ClearInput,

        // Paste
        KeyEvent {
            code: KeyCode::Char('v'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Action::Paste,

        // Scroll
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => Action::PageUp,
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => Action::PageDown,

        // Character input
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::SHIFT,
            ..
        } => Action::Char(c),

        KeyEvent {
            code: KeyCode::Tab,
            modifiers: KeyModifiers::NONE,
            ..
        } => Action::Char('\t'),

        _ => Action::None,
    }
}

/// Key bindings configuration
#[derive(Debug, Clone)]
pub struct KeyBindings {
    pub quit: Vec<KeyEvent>,
    pub submit: Vec<KeyEvent>,
    pub cancel: Vec<KeyEvent>,
    pub newline: Vec<KeyEvent>,
    pub model_selector: Vec<KeyEvent>,
    pub session_list: Vec<KeyEvent>,
    pub new_session: Vec<KeyEvent>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: vec![
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            ],
            submit: vec![KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)],
            cancel: vec![KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)],
            newline: vec![
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
                KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            ],
            model_selector: vec![],
            session_list: vec![],
            new_session: vec![],
        }
    }
}
