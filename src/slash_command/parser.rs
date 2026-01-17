use anyhow::{bail, Result};
use regex::Regex;

/// Parsed slash command with name and arguments
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: String,
}

impl ParsedCommand {
    /// Parse a slash command from user input
    /// Returns None if the input doesn't start with '/'
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        // Remove leading slash
        let without_slash = &trimmed[1..];

        // Split on first whitespace to separate command from arguments
        let mut parts = without_slash.splitn(2, char::is_whitespace);
        let name = parts.next()?.to_string();
        let args = parts.next().unwrap_or("").trim().to_string();

        Some(ParsedCommand { name, args })
    }

    /// Parse arguments into a vector of strings, respecting quotes
    pub fn parse_args(&self) -> Vec<String> {
        parse_quoted_args(&self.args)
    }
}

/// Parse arguments respecting single and double quotes
/// Examples:
///   "foo bar" -> ["foo", "bar"]
///   "foo 'bar baz'" -> ["foo", "bar baz"]
///   "foo \"bar baz\"" -> ["foo", "bar baz"]
pub fn parse_quoted_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '\\' if in_single_quote || in_double_quote => {
                // Handle escape sequences
                if let Some(&next) = chars.peek() {
                    chars.next(); // consume the next character
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            c if c.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            c => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Expand template placeholders with arguments
/// Supports: $1, $2, ..., $N for positional args and $ARGUMENTS for all args
pub fn expand_template(template: &str, args: &[String]) -> String {
    let mut result = template.to_string();

    // Replace $ARGUMENTS with all arguments joined by space
    let all_args = args.join(" ");
    result = result.replace("$ARGUMENTS", &all_args);

    // Find the last placeholder number to implement "swallow remaining args"
    // behavior for the last placeholder
    let re = Regex::new(r"\$(\d+)").unwrap();
    let last_placeholder_num = re
        .captures_iter(template)
        .filter_map(|cap| cap.get(1))
        .filter_map(|m| m.as_str().parse::<usize>().ok())
        .max();

    // Replace positional arguments $1, $2, etc.
    for (i, _) in args.iter().enumerate() {
        let placeholder_num = i + 1;
        let placeholder = format!("${}", placeholder_num);

        // For the last placeholder, swallow all remaining args
        if Some(placeholder_num) == last_placeholder_num && placeholder_num <= args.len() {
            let remaining_args: Vec<_> = args.iter().skip(i).collect();
            let remaining = remaining_args
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            result = result.replace(&placeholder, &remaining);
        } else if placeholder_num <= args.len() {
            result = result.replace(&placeholder, &args[i]);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command() {
        assert_eq!(
            ParsedCommand::parse("/help"),
            Some(ParsedCommand {
                name: "help".to_string(),
                args: "".to_string()
            })
        );

        assert_eq!(
            ParsedCommand::parse("/echo hello world"),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: "hello world".to_string()
            })
        );

        assert_eq!(
            ParsedCommand::parse("  /cmd  arg1 arg2  "),
            Some(ParsedCommand {
                name: "cmd".to_string(),
                args: "arg1 arg2".to_string()
            })
        );

        assert_eq!(ParsedCommand::parse("not a command"), None);
        assert_eq!(ParsedCommand::parse(""), None);
    }

    #[test]
    fn test_parse_quoted_args() {
        assert_eq!(parse_quoted_args("foo bar baz"), vec!["foo", "bar", "baz"]);

        assert_eq!(
            parse_quoted_args("foo 'bar baz' qux"),
            vec!["foo", "bar baz", "qux"]
        );

        assert_eq!(
            parse_quoted_args(r#"foo "bar baz" qux"#),
            vec!["foo", "bar baz", "qux"]
        );

        assert_eq!(
            parse_quoted_args(r#"foo 'it\'s' bar"#),
            vec!["foo", "it's", "bar"]
        );

        assert_eq!(parse_quoted_args(""), Vec::<String>::new());
    }

    #[test]
    fn test_expand_template() {
        assert_eq!(
            expand_template("Hello $1!", &["World".to_string()]),
            "Hello World!"
        );

        assert_eq!(
            expand_template(
                "$1 $2 $3",
                &["a".to_string(), "b".to_string(), "c".to_string()]
            ),
            "a b c"
        );

        assert_eq!(
            expand_template("All args: $ARGUMENTS", &["a".to_string(), "b".to_string()]),
            "All args: a b"
        );

        // Test last placeholder swallows remaining
        assert_eq!(
            expand_template(
                "First: $1, Rest: $2",
                &[
                    "one".to_string(),
                    "two".to_string(),
                    "three".to_string(),
                    "four".to_string()
                ]
            ),
            "First: one, Rest: two three four"
        );
    }
}
