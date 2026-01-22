//! TUI theme definitions.

use ratatui::style::{Color, Style};

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
    pub user_bg: Color,
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
            user_bg: Color::Rgb(80, 55, 35),
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
            user_bg: Color::Rgb(255, 230, 200),
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
}
