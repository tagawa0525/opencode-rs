//! Terminal User Interface module using ratatui.
//!
//! This provides an interactive chat interface similar to opencode-ts's TUI.

mod app;
mod autocomplete;
mod components;
mod dialog;
mod input;
mod llm_streaming;
mod model;
mod oauth_flow;
mod state;
mod theme;
mod types;
mod ui;

pub use app::run;
pub use state::App;
pub use types::*;
