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
    // Try each category of keys in order
    check_quit_keys(&key)
        .or_else(|| check_enter_keys(&key))
        .or_else(|| check_navigation_keys(&key))
        .or_else(|| check_editing_keys(&key))
        .or_else(|| check_control_keys(&key))
        .or_else(|| check_char_keys(&key))
        .unwrap_or(Action::None)
}

/// Check for quit key combinations
fn check_quit_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(Action::Quit),
        _ => None,
    }
}

/// Check for enter key combinations
fn check_enter_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        // Submit (Enter)
        KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Submit),
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
        } => Some(Action::Newline),
        // Cancel (Escape)
        KeyEvent {
            code: KeyCode::Esc, ..
        } => Some(Action::Cancel),
        _ => None,
    }
}

/// Check for navigation keys
fn check_navigation_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Up),
        KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Down),
        KeyEvent {
            code: KeyCode::Left,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Left),
        KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Right),
        KeyEvent {
            code: KeyCode::Home,
            ..
        } => Some(Action::Home),
        KeyEvent {
            code: KeyCode::End, ..
        } => Some(Action::End),
        KeyEvent {
            code: KeyCode::PageUp,
            ..
        } => Some(Action::PageUp),
        KeyEvent {
            code: KeyCode::PageDown,
            ..
        } => Some(Action::PageDown),
        _ => None,
    }
}

/// Check for editing keys
fn check_editing_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => Some(Action::Backspace),
        KeyEvent {
            code: KeyCode::Delete,
            ..
        } => Some(Action::Delete),
        _ => None,
    }
}

/// Check for control key combinations
fn check_control_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        // Line navigation shortcuts
        KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(Action::Home),
        KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(Action::End),
        // Clear input
        KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(Action::ClearInput),
        // Paste
        KeyEvent {
            code: KeyCode::Char('v'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => Some(Action::Paste),
        _ => None,
    }
}

/// Check for character input keys
fn check_char_keys(key: &KeyEvent) -> Option<Action> {
    match key {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::SHIFT,
            ..
        } => Some(Action::Char(*c)),
        KeyEvent {
            code: KeyCode::Tab,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(Action::Char('\t')),
        _ => None,
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
