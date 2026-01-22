//! Model-aware utility functions for tools.
//!
//! This module provides common utilities for tools that need to adapt
//! their behavior based on the model's context size.

use super::*;
use crate::provider;

/// Get model context size from models.dev
pub async fn get_model_context_size(model_id: &str) -> Result<u64> {
    // Load models from models.dev
    let providers: std::collections::HashMap<String, provider::ModelsDevProvider> =
        provider::get().await?;

    // Search for the model across all providers
    for provider_info in providers.values() {
        if let Some(model) = provider_info.models.get(model_id) {
            return Ok(model.limit.context);
        }
    }

    anyhow::bail!("Model {} not found in models.dev", model_id)
}

// Constants for output limit calculation
const DEFAULT_LIMIT: usize = 2 * 1024; // 2KB
const MIN_LIMIT: usize = 1024; // 1KB
const MAX_LIMIT: usize = 16 * 1024; // 16KB
const BYTES_PER_TOKEN: usize = 4;

/// Calculate output limit from a given context size (pure function).
///
/// This is the core calculation logic, separated for testability.
fn calculate_limit_from_context_size(context_size: u64, is_in_batch: bool) -> usize {
    let percentage = if is_in_batch { 0.02 } else { 0.01 };
    let limit_tokens = (context_size as f64 * percentage) as usize;
    let limit_bytes = limit_tokens * BYTES_PER_TOKEN;
    limit_bytes.clamp(MIN_LIMIT, MAX_LIMIT)
}

/// Calculate appropriate output limit for webfetch tool.
///
/// The limit is calculated based on the model's context size:
/// - Direct calls: 1% of context (min 1KB, max 16KB)
/// - Batch calls: 2% of context (min 1KB, max 16KB)
/// - Default: 2KB if model_id is unknown
///
/// This ensures larger context models can receive more detailed content
/// while preventing payload overflow issues.
///
/// # Examples
/// - Claude Opus 4.5 (200K): direct 8KB, batch 16KB
/// - GPT-4 (128K): direct 5KB, batch 10KB
/// - Small models (32K): direct 1KB, batch 2.6KB
pub async fn calculate_webfetch_output_limit(ctx: &ToolContext, is_in_batch: bool) -> usize {
    // If no model ID is provided, use default
    let Some(model_id) = &ctx.model_id else {
        tracing::debug!("No model ID provided, using default webfetch limit (2KB)");
        return DEFAULT_LIMIT;
    };

    // Get model context size from models.dev
    match get_model_context_size(model_id).await {
        Ok(context_size) => {
            let limit = calculate_limit_from_context_size(context_size, is_in_batch);

            tracing::debug!(
                "Model {} context: {}, webfetch limit: {} bytes ({}KB), is_in_batch: {}",
                model_id,
                context_size,
                limit,
                limit / 1024,
                is_in_batch
            );

            limit
        }
        Err(e) => {
            tracing::warn!(
                "Failed to get context size for model {}: {}. Using default webfetch limit (2KB).",
                model_id,
                e
            );
            DEFAULT_LIMIT
        }
    }
}

/// Smart truncation that respects line boundaries and UTF-8 encoding.
///
/// This function truncates content at line boundaries to avoid cutting
/// in the middle of sentences. It also ensures UTF-8 safety by checking
/// character boundaries.
///
/// # Arguments
/// * `content` - The content to truncate
/// * `max_bytes` - Maximum size in bytes
///
/// # Returns
/// Truncated content that is:
/// - At most `max_bytes` in size
/// - Ends at a line boundary (unless a single line exceeds the limit)
/// - UTF-8 safe (no broken multibyte characters)
pub fn smart_truncate(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    // Find the last line boundary before max_bytes
    let mut truncate_at = 0;
    let mut current_pos = 0;

    for line in content.lines() {
        let line_len = line.len() + 1; // +1 for newline
        if current_pos + line_len > max_bytes {
            break;
        }
        current_pos += line_len;
        truncate_at = current_pos;
    }

    // If we couldn't fit even one line, truncate at max_bytes with UTF-8 safety
    if truncate_at == 0 {
        // Find the last valid UTF-8 boundary before max_bytes
        let mut pos = max_bytes.min(content.len());
        while pos > 0 && !content.is_char_boundary(pos) {
            pos -= 1;
        }
        return content[..pos].to_string();
    }

    // Return truncated content at line boundary
    content[..truncate_at].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_truncate_within_limit() {
        let content = "Hello\nWorld\n";
        let result = smart_truncate(content, 100);
        assert_eq!(result, content);
    }

    #[test]
    fn test_smart_truncate_at_line_boundary() {
        let content = "Line 1\nLine 2\nLine 3\n";
        let result = smart_truncate(content, 14); // Fits "Line 1\nLine 2\n"
        assert_eq!(result, "Line 1\nLine 2\n");
    }

    #[test]
    fn test_smart_truncate_single_long_line() {
        let content = "ThisIsAVeryLongLineThatExceedsTheLimit";
        let result = smart_truncate(content, 10);
        assert_eq!(result, "ThisIsAVer");
    }

    #[test]
    fn test_smart_truncate_utf8_safe() {
        let content = "Hello 世界\nNext line\n";
        // "Hello 世界\n" = "Hello " (6 bytes) + "世界" (6 bytes) + "\n" (1 byte) = 13 bytes
        let result = smart_truncate(content, 13);
        assert_eq!(result, "Hello 世界\n");
    }

    #[test]
    fn test_smart_truncate_utf8_boundary() {
        let content = "Hello 世界";
        // Truncate in the middle of a multibyte character
        let result = smart_truncate(content, 8); // Would be in middle of "世"
                                                 // Should find valid UTF-8 boundary (before "世")
        assert_eq!(result, "Hello ");
    }

    #[tokio::test]
    async fn test_calculate_output_limit_no_model() {
        let ctx = ToolContext::new("test-session", "test-message", "test-agent");
        let limit = calculate_webfetch_output_limit(&ctx, false).await;
        assert_eq!(limit, 2 * 1024); // Default 2KB
    }

    #[test]
    fn test_calculate_limit_from_context_size_direct_vs_batch() {
        // Test various context sizes to ensure batch >= direct always holds
        let test_cases = [
            32_000u64,   // Small model
            128_000,     // GPT-4 class
            200_000,     // Claude Opus 4.5
            500_000,     // Large context
            1_000_000,   // Very large context
        ];

        for context_size in test_cases {
            let direct = calculate_limit_from_context_size(context_size, false);
            let batch = calculate_limit_from_context_size(context_size, true);

            assert!(
                batch >= direct,
                "For context_size {}: batch ({}) should be >= direct ({})",
                context_size,
                batch,
                direct
            );
        }
    }

    #[test]
    fn test_calculate_limit_from_context_size_bounds() {
        // Test MIN_LIMIT bound (small context)
        let small_context = 10_000u64; // 10K tokens
        let direct = calculate_limit_from_context_size(small_context, false);
        assert_eq!(direct, MIN_LIMIT, "Should clamp to MIN_LIMIT for small context");

        // Test MAX_LIMIT bound (very large context)
        let large_context = 1_000_000u64; // 1M tokens
        let batch = calculate_limit_from_context_size(large_context, true);
        assert_eq!(batch, MAX_LIMIT, "Should clamp to MAX_LIMIT for large context");
    }

    #[test]
    fn test_calculate_limit_from_context_size_expected_values() {
        // Claude Opus 4.5 (200K context):
        // direct: 200,000 * 0.01 * 4 = 8,000 bytes
        // batch:  200,000 * 0.02 * 4 = 16,000 bytes
        let context_200k = 200_000u64;
        assert_eq!(calculate_limit_from_context_size(context_200k, false), 8_000);
        assert_eq!(calculate_limit_from_context_size(context_200k, true), 16_000);

        // GPT-4 class (128K context):
        // direct: 128,000 * 0.01 * 4 = 5,120 bytes
        // batch:  128,000 * 0.02 * 4 = 10,240 bytes
        let context_128k = 128_000u64;
        assert_eq!(calculate_limit_from_context_size(context_128k, false), 5_120);
        assert_eq!(calculate_limit_from_context_size(context_128k, true), 10_240);
    }
}
