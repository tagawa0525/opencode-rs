//! Model-related utilities.

/// Parse a model string in the format "provider/model"
pub fn parse_model_string(model: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = model.splitn(2, '/').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
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
}
