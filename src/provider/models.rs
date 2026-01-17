//! Model-related utilities.

use super::*;

/// Parse a model string in the format "provider/model"
pub fn parse_model_string(model: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = model.splitn(2, '/').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Format a model reference as a string
pub fn format_model_string(provider_id: &str, model_id: &str) -> String {
    format!("{}/{}", provider_id, model_id)
}

/// Get recommended models for various use cases
pub struct ModelRecommendations;

impl ModelRecommendations {
    /// Models recommended for code generation
    pub fn for_coding() -> Vec<(&'static str, &'static str)> {
        vec![
            ("copilot", "claude-sonnet-4-5-20250929"),
            ("copilot", "claude-opus-4-5-20251124"),
            ("anthropic", "claude-sonnet-4-20250514"),
            ("anthropic", "claude-3-5-sonnet-20241022"),
            ("openai", "gpt-4o"),
            ("openai", "o1"),
        ]
    }

    /// Models recommended for quick tasks (title generation, etc.)
    pub fn for_quick_tasks() -> Vec<(&'static str, &'static str)> {
        vec![
            ("anthropic", "claude-3-5-haiku-20241022"),
            ("openai", "gpt-4o-mini"),
            ("google", "gemini-2.0-flash"),
        ]
    }

    /// Models with reasoning capabilities
    pub fn with_reasoning() -> Vec<(&'static str, &'static str)> {
        vec![
            ("copilot", "claude-sonnet-4-5-20250929"),
            ("copilot", "claude-opus-4-5-20251124"),
            ("anthropic", "claude-sonnet-4-20250514"),
            ("openai", "o1"),
            ("openai", "o1-mini"),
        ]
    }
}

/// Sort models by preference for display
pub fn sort_models(models: &mut [Model]) {
    let priority = [
        "claude-sonnet-4-5",
        "claude-opus-4-5",
        "claude-sonnet-4",
        "gpt-4o",
        "gemini-2.0",
        "o1",
    ];

    models.sort_by(|a, b| {
        let a_priority = priority
            .iter()
            .position(|p| a.id.contains(p))
            .unwrap_or(usize::MAX);
        let b_priority = priority
            .iter()
            .position(|p| b.id.contains(p))
            .unwrap_or(usize::MAX);

        a_priority.cmp(&b_priority).then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_string() {
        assert_eq!(
            parse_model_string("anthropic/claude-3"),
            Some(("anthropic".to_string(), "claude-3".to_string()))
        );
        assert_eq!(parse_model_string("invalid"), None);
        assert_eq!(
            parse_model_string("provider/model/with/slashes"),
            Some(("provider".to_string(), "model/with/slashes".to_string()))
        );
    }

    #[test]
    fn test_format_model_string() {
        assert_eq!(
            format_model_string("anthropic", "claude-3"),
            "anthropic/claude-3"
        );
    }
}
