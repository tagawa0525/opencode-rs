//! Terminal User Interface module using ratatui.
//!
//! This provides an interactive chat interface similar to opencode-ts's TUI.

mod app;
mod autocomplete;
mod clipboard;
mod command_handler;
mod components;
mod dialog;
mod input;
mod llm_streaming;
mod model;
mod oauth_flow;
mod state;
mod theme;
mod transcript;
mod types;
mod ui;

pub use app::run;
pub use clipboard::copy_to_clipboard;
pub use state::App;
pub use transcript::{format_transcript, TranscriptOptions};
pub use types::*;
