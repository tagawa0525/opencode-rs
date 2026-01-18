use opencode::slash_command::loader::load_commands_from_directory;
use std::path::PathBuf;

#[tokio::test]
async fn test_load_commands_from_opencode_directory() {
    let base_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let commands = load_commands_from_directory(&base_path)
        .await
        .expect("Failed to load commands");

    println!("Loaded {} commands from .opencode/command/", commands.len());

    // We should have at least the test command we created
    assert!(commands.len() > 0, "Should load at least one command");

    // Print all loaded commands
    for cmd in &commands {
        println!("  - {} : {}", cmd.name(), cmd.description());
    }

    // Check if our test command is loaded
    let test_cmd = commands.iter().find(|cmd| cmd.name() == "test-rust");
    assert!(test_cmd.is_some(), "test-rust command should be loaded");

    if let Some(cmd) = test_cmd {
        assert_eq!(cmd.description(), "Test command for Rust implementation");
    }

    // Check if existing commands are loaded
    let commit_cmd = commands.iter().find(|cmd| cmd.name() == "commit");
    assert!(commit_cmd.is_some(), "commit command should be loaded");

    if let Some(cmd) = commit_cmd {
        assert_eq!(cmd.description(), "git commit and push");
    }
}
