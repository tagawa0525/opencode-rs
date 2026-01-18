use opencode::slash_command::parser::{
    expand_template, expand_template_async, extract_file_references,
};

#[test]
fn test_expand_template_with_arguments() {
    let template = "Explain $1 in the context of $2";
    let args = vec!["Rust".to_string(), "web development".to_string()];

    let result = expand_template(template, &args);
    assert_eq!(result, "Explain Rust in the context of web development");
}

#[test]
fn test_expand_template_with_all_arguments() {
    let template = "Process these files: $ARGUMENTS";
    let args = vec![
        "file1.rs".to_string(),
        "file2.rs".to_string(),
        "file3.rs".to_string(),
    ];

    let result = expand_template(template, &args);
    assert_eq!(result, "Process these files: file1.rs file2.rs file3.rs");
}

#[tokio::test]
async fn test_expand_template_async_with_shell_command() {
    let template = "Current directory: !`pwd`";

    let result = expand_template_async(template, &[]).await;
    assert!(result.is_ok(), "Shell command expansion should succeed");

    let expanded = result.unwrap();
    assert!(expanded.contains("Current directory:"));
    assert!(!expanded.contains("!`"));
}

#[test]
fn test_extract_file_references_basic() {
    let template = "Check @README.md for details";
    let files = extract_file_references(template);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0], "README.md");
}

#[test]
fn test_extract_file_references_multiple() {
    let template = "Review @src/main.rs and @Cargo.toml";
    let files = extract_file_references(template);

    assert_eq!(files.len(), 2);
    assert!(files.contains(&"src/main.rs".to_string()));
    assert!(files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn test_extract_file_references_with_home() {
    let template = "Check config at @~/.config/opencode.json";
    let files = extract_file_references(template);

    assert_eq!(files.len(), 1);
    assert_eq!(files[0], "~/.config/opencode.json");
}

#[test]
fn test_expand_template_last_arg_captures_rest() {
    let template = "First: $1, Rest: $2";
    let args = vec![
        "one".to_string(),
        "two".to_string(),
        "three".to_string(),
        "four".to_string(),
    ];

    let result = expand_template(template, &args);
    assert_eq!(result, "First: one, Rest: two three four");
}
