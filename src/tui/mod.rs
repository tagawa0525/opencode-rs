//! Terminal User Interface module using ratatui.
//!
//! This provides an interactive chat interface similar to opencode-ts's TUI.

mod app;
mod components;
mod input;
mod theme;
mod ui;

pub use app::run;
