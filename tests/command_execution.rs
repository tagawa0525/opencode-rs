use opencode::slash_command::{
    loader::load_commands_from_directory, registry::CommandRegistry, CommandContext,
};
use std::collections::HashMap;
use std::path::PathBuf;

#[tokio::test]
async fn test_execute_markdown_command() {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Load commands from markdown files
    let commands = load_commands_from_directory(&base_path)
        .await
        .expect("Failed to load commands");

    // Create registry and register commands
    let registry = CommandRegistry::new();
    for cmd in commands {
        registry.register(cmd).await;
    }

    // Create context
    let ctx = CommandContext {
        session_id: "test-session".to_string(),
        cwd: base_path.to_str().unwrap().to_string(),
        root: base_path.to_str().unwrap().to_string(),
        extra: HashMap::new(),
    };

    // Execute test-rust command
    let result = registry
        .execute("test-rust", "hello world", &ctx)
        .await
        .expect("Command execution should succeed");

    println!("Command output:\n{}", result.text);

    // Verify output contains expected content
    assert!(result
        .text
        .contains("This is a test command for the Rust version"));
    assert!(result.text.contains("Arguments provided: hello world"));
    assert!(result.text.contains("First arg: hello"));
    assert!(result.text.contains("Second arg: world"));
    assert!(result.submit_to_llm, "Should submit to LLM");
}

#[tokio::test]
async fn test_execute_command_with_model_override() {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Load commands
    let commands = load_commands_from_directory(&base_path)
        .await
        .expect("Failed to load commands");

    // Create registry
    let registry = CommandRegistry::new();
    for cmd in commands {
        registry.register(cmd).await;
    }

    // Create context
    let ctx = CommandContext {
        session_id: "test-session".to_string(),
        cwd: base_path.to_str().unwrap().to_string(),
        root: base_path.to_str().unwrap().to_string(),
        extra: HashMap::new(),
    };

    // Execute commit command (which has model override)
    let result = registry
        .execute("commit", "", &ctx)
        .await
        .expect("Command execution should succeed");

    println!("Commit command output:\n{}", result.text);

    // Verify model override
    assert_eq!(result.model, Some("opencode/glm-4.6".to_string()));
    assert!(result.text.contains("commit and push"));
}

#[tokio::test]
async fn test_list_all_commands() {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Load commands
    let commands = load_commands_from_directory(&base_path)
        .await
        .expect("Failed to load commands");

    // Create registry
    let registry = CommandRegistry::new();
    for cmd in commands {
        registry.register(cmd).await;
    }

    // List all commands
    let command_list = registry.list().await;

    println!("\nAvailable commands:");
    for cmd_info in &command_list {
        println!("  /{} - {}", cmd_info.name, cmd_info.description);
    }

    assert!(command_list.len() >= 6, "Should have at least 6 commands");

    // Verify specific commands exist
    assert!(command_list.iter().any(|c| c.name == "test-rust"));
    assert!(command_list.iter().any(|c| c.name == "commit"));
    assert!(command_list.iter().any(|c| c.name == "spellcheck"));
}
