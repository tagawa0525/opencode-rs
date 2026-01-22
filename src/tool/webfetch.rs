//! WebFetch tool - fetches content from URLs.
//!
//! This tool allows the LLM to retrieve web content and convert it to
//! various formats (markdown, text, or HTML).

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

const DESCRIPTION: &str = r#"- Fetches content from a specified URL
- Takes a URL and optional format as input
- Fetches the URL content, converts to requested format (markdown by default)
- Returns the content in the specified format
- Use this tool when you need to retrieve and analyze web content

Usage notes:
  - IMPORTANT: if another tool is present that offers better web fetching capabilities, is more targeted to the task, or has fewer restrictions, prefer using that tool instead of this one.
  - IMPORTANT: If you need to fetch many URLs (5+), use the 'batch' tool to avoid context overflow.
  - Results are automatically truncated based on model context size (typically 2-16KB)
  - Larger context models receive more detailed content
  - The URL must be a fully-formed valid URL
  - HTTP URLs will be automatically upgraded to HTTPS
  - Format options: "markdown" (default), "text", or "html"
  - This tool is read-only and does not modify any files
"#;

const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024; // 5MB
const DEFAULT_TIMEOUT: u64 = 30; // 30 seconds
const MAX_TIMEOUT: u64 = 120; // 2 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchParams {
    pub url: String,
    #[serde(default = "default_format")]
    pub format: String,
    pub timeout: Option<u64>,
}

fn default_format() -> String {
    "markdown".to_string()
}

pub struct WebFetchTool;

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "webfetch".to_string(),
            description: DESCRIPTION.to_string(),
            parameters: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch content from"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "markdown", "html"],
                        "default": "markdown",
                        "description": "The format to return the content in (text, markdown, or html). Defaults to markdown."
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Optional timeout in seconds (max 120)"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let params: WebFetchParams = serde_json::from_value(args)?;

        // Validate URL
        if !params.url.starts_with("http://") && !params.url.starts_with("https://") {
            return Ok(ToolResult::error(
                "Invalid URL",
                "URL must start with http:// or https://",
            ));
        }

        // Upgrade HTTP to HTTPS
        let url = if params.url.starts_with("http://") {
            params.url.replace("http://", "https://")
        } else {
            params.url.clone()
        };

        // Request permission before fetching
        let mut metadata = HashMap::from([
            ("url".to_string(), json!(url)),
            ("format".to_string(), json!(params.format)),
        ]);
        if let Some(timeout) = params.timeout {
            metadata.insert("timeout".to_string(), json!(timeout));
        }

        // Extract domain from URL for pattern matching
        let domain_pattern = extract_domain_pattern(&url);

        let allowed = ctx
            .ask_permission(
                "webfetch".to_string(),
                vec![url.clone()],
                vec![domain_pattern, "*".to_string()],
                metadata,
            )
            .await?;

        if !allowed {
            return Ok(ToolResult::error(
                "Permission Denied",
                format!("User denied permission to fetch URL: {}", url),
            ));
        }

        // Set timeout
        let timeout_secs = params.timeout.unwrap_or(DEFAULT_TIMEOUT).min(MAX_TIMEOUT);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        // Build Accept header based on requested format
        let accept_header = match params.format.as_str() {
            "markdown" => {
                "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1"
            }
            "text" => {
                "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1"
            }
            "html" => {
                "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, text/markdown;q=0.7, */*;q=0.1"
            }
            _ => "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        };

        // Create HTTP client with timeout
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36")
            .build()?;

        // Check for abort signal
        if ctx.is_aborted() {
            return Ok(ToolResult::error("Aborted", "Request was aborted"));
        }

        // Fetch the URL
        let response = client
            .get(&url)
            .header("Accept", accept_header)
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Request failed: {}", e))?;

        if !response.status().is_success() {
            return Ok(ToolResult::error(
                "Request failed",
                format!("HTTP status code: {}", response.status()),
            ));
        }

        // Check content length
        if let Some(content_length) = response.content_length() {
            if content_length as usize > MAX_RESPONSE_SIZE {
                return Ok(ToolResult::error(
                    "Response too large",
                    "Response exceeds 5MB limit",
                ));
            }
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Read response body
        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

        if bytes.len() > MAX_RESPONSE_SIZE {
            return Ok(ToolResult::error(
                "Response too large",
                "Response exceeds 5MB limit",
            ));
        }

        // Convert to string
        let content = String::from_utf8_lossy(&bytes).to_string();

        let title = format!("{} ({})", params.url, content_type);

        // Handle content based on requested format and actual content type
        let mut output = match params.format.as_str() {
            "markdown" => {
                if content_type.contains("text/html") {
                    convert_html_to_markdown(&content)
                } else {
                    content
                }
            }
            "text" => {
                if content_type.contains("text/html") {
                    extract_text_from_html(&content)
                } else {
                    content
                }
            }
            _ => content,
        };

        // Truncate output to prevent payload overflow
        // Dynamically calculate limit based on model context size
        // Check if we're running in a batch to apply appropriate limits
        let is_in_batch = ctx
            .extra
            .get("is_in_batch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Calculate output limit based on model context size
        let max_output_size = calculate_webfetch_output_limit(ctx, is_in_batch).await;

        if output.len() > max_output_size {
            // Use smart truncation to respect line boundaries and UTF-8 encoding
            output = smart_truncate(&output, max_output_size);
            let truncation_msg = format!(
                "\n\n[Output truncated: content exceeds {}KB limit. Use batch tool for multiple fetches.]",
                max_output_size / 1024
            );
            output.push_str(&truncation_msg);
        }

        Ok(ToolResult::success(title, output))
    }
}

/// Convert HTML to Markdown (simplified version)
fn convert_html_to_markdown(html: &str) -> String {
    // This is a very basic implementation
    // TODO: Use a proper HTML-to-Markdown converter like html2md or pulldown-cmark

    // Remove script and style tags
    let mut result = html.to_string();

    // Simple regex-like replacements (very basic)
    result = result
        .replace("<script", "\n<script")
        .replace("</script>", "</script>\n")
        .replace("<style", "\n<style")
        .replace("</style>", "</style>\n");

    // Remove script and style content
    let mut clean = String::new();
    let mut in_script = false;
    let mut in_style = false;

    for line in result.lines() {
        if line.contains("<script") {
            in_script = true;
        } else if line.contains("</script>") {
            in_script = false;
            continue;
        } else if line.contains("<style") {
            in_style = true;
        } else if line.contains("</style>") {
            in_style = false;
            continue;
        }

        if !in_script && !in_style {
            clean.push_str(line);
            clean.push('\n');
        }
    }

    // Basic HTML tag removal
    let re_tag = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re_tag.replace_all(&clean, " ");

    // Clean up whitespace
    let re_space = regex::Regex::new(r"\s+").unwrap();
    let cleaned = re_space.replace_all(&text, " ");

    cleaned.trim().to_string()
}

/// Extract text from HTML (simplified version)
fn extract_text_from_html(html: &str) -> String {
    // For now, use the same implementation as markdown conversion
    // TODO: Implement proper text extraction
    convert_html_to_markdown(html)
}

/// Extract domain pattern from URL for permission matching
/// e.g., "https://crates.io/api/v1/crates/tokio" -> "https://crates.io/*"
fn extract_domain_pattern(url: &str) -> String {
    // Simple regex-free approach
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            let scheme = &url[..scheme_end];
            let domain = &after_scheme[..path_start];
            return format!("{}://{}/*", scheme, domain);
        } else {
            // No path, just domain
            return format!("{}/*", url);
        }
    }
    // Fallback: just return the URL itself
    url.to_string()
}
