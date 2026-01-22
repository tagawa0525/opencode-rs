//! Grep tool for searching file contents.

use super::*;
use ::glob::Pattern;
use anyhow::Result;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};

/// Maximum number of search results
const MAX_RESULTS: usize = 500;
/// Maximum matches per file
const MAX_MATCHES_PER_FILE: usize = 10;

/// Binary file extensions to skip
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "woff", "woff2", "ttf", "eot", "pdf", "zip", "tar", "gz",
    "exe", "dll", "so", "dylib", "bin", "dat",
];

/// Tool for searching file contents with regex
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

/// Arguments parsed from the tool input
struct GrepArgs {
    pattern: String,
    search_path: String,
    include_pattern: Option<String>,
}

/// A single search result
#[derive(Debug)]
struct SearchResult {
    path: String,
    line_number: usize,
    line_content: String,
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: r#"Fast content search tool that works with any codebase size.
- Searches file contents using regular expressions
- Supports full regex syntax (e.g., "log.*Error", "function\s+\w+")
- Filter files by pattern with the include parameter (e.g., "*.rs", "*.{ts,tsx}")
- Returns file paths and line numbers with matches
- Respects .gitignore by default"#
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The regex pattern to search for in file contents"
                    },
                    "path": {
                        "type": "string",
                        "description": "The directory to search in (defaults to current directory)"
                    },
                    "include": {
                        "type": "string",
                        "description": "File pattern to include (e.g., \"*.rs\", \"*.{ts,tsx}\")"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let args = parse_args(args, ctx)?;

        // Request permission before grepping
        let mut metadata = HashMap::from([
            ("pattern".to_string(), json!(args.pattern)),
            ("path".to_string(), json!(args.search_path)),
        ]);
        if let Some(ref include) = args.include_pattern {
            metadata.insert("include".to_string(), json!(include));
        }

        if let Some(denied) = ctx
            .require_permission("grep", vec![args.pattern.clone()], metadata)
            .await?
        {
            return Ok(denied);
        }

        // Compile regex
        let regex = Regex::new(&args.pattern)
            .map_err(|e| anyhow::anyhow!("Invalid regex pattern '{}': {}", args.pattern, e))?;

        // Build glob matcher for include pattern
        let include_glob = args
            .include_pattern
            .as_ref()
            .and_then(|p| Pattern::new(p).ok());

        // Search files
        let results = search_files(&args.search_path, &regex, &include_glob);

        // Build result
        Ok(build_result(&args, results))
    }
}

/// Parse arguments from the tool input
fn parse_args(args: Value, ctx: &ToolContext) -> Result<GrepArgs> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("pattern is required"))?
        .to_string();

    // Resolve search path using context helper
    let search_path_arg = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(&ctx.cwd);

    let search_path = ctx.resolve_path(search_path_arg).to_string_lossy().to_string();

    let include_pattern = args
        .get("include")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(GrepArgs {
        pattern,
        search_path,
        include_pattern,
    })
}

/// Search files for regex matches
fn search_files(
    search_path: &str,
    regex: &Regex,
    include_glob: &Option<Pattern>,
) -> Vec<SearchResult> {
    let walker = WalkBuilder::new(search_path)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .build();

    let mut results: Vec<SearchResult> = Vec::new();

    'outer: for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if !should_search_file(path, include_glob) {
            continue;
        }

        // Read file content
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Search for matches
        let relative_path = path
            .strip_prefix(search_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());

        let mut file_matches = 0;
        for (line_num, line) in content.lines().enumerate() {
            if regex.is_match(line) {
                results.push(SearchResult {
                    path: relative_path.clone(),
                    line_number: line_num + 1,
                    line_content: line.chars().take(200).collect(),
                });

                file_matches += 1;
                if file_matches >= MAX_MATCHES_PER_FILE {
                    break;
                }

                if results.len() >= MAX_RESULTS {
                    break 'outer;
                }
            }
        }
    }

    // Sort by path, then line number
    results.sort_by(|a, b| a.path.cmp(&b.path).then(a.line_number.cmp(&b.line_number)));

    results
}

/// Check if a file should be searched
fn should_search_file(path: &std::path::Path, include_glob: &Option<Pattern>) -> bool {
    // Skip directories
    if !path.is_file() {
        return false;
    }

    // Check include pattern
    if let Some(ref glob) = include_glob {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !glob.matches(filename) {
            return false;
        }
    }

    // Skip binary files
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    !BINARY_EXTENSIONS.contains(&ext.as_str())
}

/// Build the tool result from search results
fn build_result(args: &GrepArgs, results: Vec<SearchResult>) -> ToolResult {
    let total_count = results.len();
    let truncated = total_count >= MAX_RESULTS;

    let output = if results.is_empty() {
        format!(
            "No matches found for pattern '{}' in {}",
            args.pattern, args.search_path
        )
    } else {
        results
            .iter()
            .map(|r| format!("{}:{}: {}", r.path, r.line_number, r.line_content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let title = if total_count == 0 {
        format!("No matches for '{}'", args.pattern)
    } else if truncated {
        format!("Found {}+ matches for '{}'", total_count, args.pattern)
    } else {
        format!("Found {} matches for '{}'", total_count, args.pattern)
    };

    ToolResult {
        title,
        output,
        metadata: build_metadata(args, total_count, truncated),
        truncated,
        attachments: Vec::new(),
    }
}

/// Build metadata for the result
fn build_metadata(args: &GrepArgs, count: usize, truncated: bool) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("pattern".to_string(), json!(&args.pattern));
    m.insert("path".to_string(), json!(&args.search_path));
    m.insert("count".to_string(), json!(count));
    m.insert("truncated".to_string(), json!(truncated));
    if let Some(ref include) = args.include_pattern {
        m.insert("include".to_string(), json!(include));
    }
    m
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}
