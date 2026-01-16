//! TUI theme definitions.

use ratatui::style::{Color, Modifier, Style};

/// Theme configuration for the TUI
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,

    // Base colors
    pub background: Color,
    pub foreground: Color,
    pub dim: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,

    // UI element colors
    pub border: Color,
    pub border_focused: Color,
    pub selection: Color,
    pub cursor: Color,

    // Message colors
    pub user_message: Color,
    pub assistant_message: Color,
    pub system_message: Color,
    pub tool_message: Color,

    // Syntax highlighting
    pub syntax_keyword: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_comment: Color,
    pub syntax_function: Color,
    pub syntax_type: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Dark theme (default)
    pub fn dark() -> Self {
        Self {
            name: "dark".to_string(),

            background: Color::Rgb(30, 30, 30),
            foreground: Color::Rgb(220, 220, 220),
            dim: Color::Rgb(128, 128, 128),
            accent: Color::Rgb(138, 180, 248),
            error: Color::Rgb(244, 135, 135),
            warning: Color::Rgb(255, 200, 100),
            success: Color::Rgb(144, 238, 144),
            info: Color::Rgb(135, 206, 250),

            border: Color::Rgb(60, 60, 60),
            border_focused: Color::Rgb(138, 180, 248),
            selection: Color::Rgb(60, 80, 120),
            cursor: Color::Rgb(255, 255, 255),

            user_message: Color::Rgb(180, 220, 255),
            assistant_message: Color::Rgb(220, 220, 220),
            system_message: Color::Rgb(180, 180, 180),
            tool_message: Color::Rgb(200, 180, 255),

            syntax_keyword: Color::Rgb(198, 120, 221),
            syntax_string: Color::Rgb(152, 195, 121),
            syntax_number: Color::Rgb(209, 154, 102),
            syntax_comment: Color::Rgb(128, 128, 128),
            syntax_function: Color::Rgb(97, 175, 239),
            syntax_type: Color::Rgb(229, 192, 123),
        }
    }

    /// Light theme
    pub fn light() -> Self {
        Self {
            name: "light".to_string(),

            background: Color::Rgb(250, 250, 250),
            foreground: Color::Rgb(40, 40, 40),
            dim: Color::Rgb(140, 140, 140),
            accent: Color::Rgb(0, 100, 200),
            error: Color::Rgb(200, 50, 50),
            warning: Color::Rgb(200, 150, 0),
            success: Color::Rgb(50, 150, 50),
            info: Color::Rgb(50, 100, 200),

            border: Color::Rgb(200, 200, 200),
            border_focused: Color::Rgb(0, 100, 200),
            selection: Color::Rgb(200, 220, 255),
            cursor: Color::Rgb(0, 0, 0),

            user_message: Color::Rgb(0, 80, 160),
            assistant_message: Color::Rgb(40, 40, 40),
            system_message: Color::Rgb(100, 100, 100),
            tool_message: Color::Rgb(100, 50, 150),

            syntax_keyword: Color::Rgb(160, 60, 180),
            syntax_string: Color::Rgb(60, 140, 60),
            syntax_number: Color::Rgb(180, 100, 40),
            syntax_comment: Color::Rgb(140, 140, 140),
            syntax_function: Color::Rgb(40, 100, 200),
            syntax_type: Color::Rgb(180, 130, 40),
        }
    }

    /// Get style for text
    pub fn text(&self) -> Style {
        Style::default().fg(self.foreground)
    }

    /// Get style for dimmed text
    pub fn text_dim(&self) -> Style {
        Style::default().fg(self.dim)
    }

    /// Get style for accent text
    pub fn text_accent(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Get style for error text
    pub fn text_error(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Get style for borders
    pub fn border(&self, focused: bool) -> Style {
        Style::default().fg(if focused {
            self.border_focused
        } else {
            self.border
        })
    }

    /// Get style for selection
    pub fn selection(&self) -> Style {
        Style::default().bg(self.selection)
    }

    /// Get style for user messages
    pub fn user(&self) -> Style {
        Style::default()
            .fg(self.user_message)
            .add_modifier(Modifier::BOLD)
    }

    /// Get style for assistant messages
    pub fn assistant(&self) -> Style {
        Style::default().fg(self.assistant_message)
    }

    /// Get style for tool output
    pub fn tool(&self) -> Style {
        Style::default().fg(self.tool_message)
    }
}
