//! Autocomplete handling for slash commands.
//!
//! This module contains autocomplete-related methods for the App.
//! Similar to component/prompt/autocomplete.tsx in the TS version.

use super::state::App;
use super::types::{AutocompleteState, CommandItem};

/// Autocomplete-related methods for App
impl App {
    /// Show autocomplete for slash commands
    pub async fn show_autocomplete(&mut self, filter: &str) {
        use fuzzy_matcher::FuzzyMatcher;

        let commands = self.command_registry.list().await;
        let mut items: Vec<CommandItem> = commands
            .into_iter()
            .map(|cmd| CommandItem {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
                display: format!("/{}", cmd.name),
            })
            .collect();

        // Apply fuzzy filtering if there's a filter
        if !filter.is_empty() {
            let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
            let mut scored_items: Vec<(i64, CommandItem)> = items
                .into_iter()
                .filter_map(|item| {
                    let score = matcher.fuzzy_match(&item.name, filter)?;
                    Some((score, item))
                })
                .collect();

            // Sort by score (descending)
            scored_items.sort_by(|a, b| b.0.cmp(&a.0));
            items = scored_items.into_iter().map(|(_, item)| item).collect();
        }

        // Limit to 10 items
        items.truncate(10);

        if !items.is_empty() {
            let mut state = AutocompleteState::new(items);
            state.filter = filter.to_string();
            self.autocomplete = Some(state);
        } else {
            self.autocomplete = None;
        }
    }

    /// Update autocomplete based on current input
    pub async fn update_autocomplete(&mut self) {
        // Check if input starts with "/" and cursor is at a position where autocomplete makes sense
        if self.input.starts_with('/') {
            // Find the filter text (everything after / until cursor or first space)
            let cursor_pos = self.cursor_position.min(self.input.len());
            let input_until_cursor = self.input[..cursor_pos].to_string();

            // If there's a space before cursor, hide autocomplete
            if input_until_cursor.contains(' ') {
                self.hide_autocomplete();
                return;
            }

            // Extract filter (text after /)
            let filter = input_until_cursor[1..].to_string(); // Remove leading /
            self.show_autocomplete(&filter).await;
        } else {
            self.hide_autocomplete();
        }
    }

    /// Insert selected autocomplete item and return the command name
    pub fn insert_autocomplete_selection(&mut self) -> Option<String> {
        if let Some(autocomplete) = &self.autocomplete {
            if let Some(item) = autocomplete.selected_item() {
                let command_name = item.name.clone();
                self.hide_autocomplete();
                // Clear the input - we'll execute the command directly
                self.input.clear();
                self.cursor_position = 0;
                return Some(command_name);
            }
        }
        None
    }
}
