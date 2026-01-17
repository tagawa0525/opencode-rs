# opencode-rs

A Rust implementation of opencode - an AI-powered development tool.

**⚡ Quick Start**: See [QUICKSTART.md](QUICKSTART.md) to get started in 3 steps!

## Overview

opencode-rs is a Rust port of [opencode-ts](https://github.com/anomalyco/opencode), providing a fast, efficient CLI tool for AI-assisted development with support for multiple LLM providers.

## Features

- 🚀 **Fast & Efficient**: Written in Rust for optimal performance
- 🤖 **Multiple LLM Providers**: Support for Anthropic, OpenAI, and more
- 🛠️ **Tool Integration**: Built-in tools for file operations, code search, shell commands
- 💬 **Interactive TUI**: Terminal user interface for chat-based development
- 📝 **CLI Mode**: Non-interactive prompt mode for scripting
- ⚙️ **Configurable**: JSON-based configuration with environment variable support

## Installation

### From Source

```bash
git clone https://github.com/your-repo/opencode-rs
cd opencode-rs
cargo build --release
```

The binary will be available at `target/release/opencode`.

### Add to PATH

```bash
# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)
export PATH="$PATH:/path/to/opencode-rs/target/release"
```

## Quick Start

### 1. Initialize Configuration

```bash
opencode config init
```

This creates a default configuration file at `~/.config/opencode/opencode.json`.

### 2. Configure API Keys

Edit the configuration file to add your API keys:

```json
{
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    },
    "openai": {
      "key": "$OPENAI_API_KEY"
    },
    "copilot": {
      "key": "$GITHUB_COPILOT_TOKEN"
    }
  },
  "model": "copilot/claude-sonnet-4-5-20250929"
}
```

You can use environment variables directly in the config by prefixing with `$`.

### 3. Set Environment Variables

```bash
export ANTHROPIC_API_KEY="your-api-key-here"
# or
export OPENAI_API_KEY="your-api-key-here"
```

## Usage

### Interactive TUI Mode

Start an interactive terminal session:

```bash
opencode
# or
opencode run
```

This launches a full-screen TUI where you can chat with the AI assistant.

**Important**: TUI mode requires an interactive terminal (TTY). This means:
- ✅ Works in: Terminal emulators (iTerm2, Terminal.app, GNOME Terminal, etc.)
- ❌ Does not work in: CI/CD pipelines, pipes, background jobs, `cargo run` in some environments
- 🔧 Alternative: Use the `prompt` command for non-interactive usage

If you see a TTY error when running `cargo run`, try running the compiled binary directly:
```bash
cargo build --release
./target/release/opencode
```

### Prompt Mode (Non-Interactive)

Send a single prompt without TUI:

```bash
opencode prompt "explain this code" --model copilot/claude-sonnet-4-5-20250929
```

Options:
- `--model, -m`: Specify the model to use (format: `provider/model`)
- `--format`: Output format (`text`, `json`, `markdown`)

Examples:

```bash
# Simple prompt
opencode prompt "what files are in this directory?"

# With specific model
opencode prompt "review this code" -m openai/gpt-4

# JSON output
opencode prompt "list all TypeScript files" --format json

# Markdown output
opencode prompt "explain the architecture" --format markdown
```

### Configuration Management

```bash
# Show current configuration
opencode config show

# Show config file paths
opencode config path

# Initialize default config
opencode config init
```

### Session Management

```bash
# List all sessions
opencode session list

# Show session details
opencode session show <session-id>

# Delete a session
opencode session delete <session-id>
```

## Configuration

The configuration is loaded from:
1. Global config: `~/.config/opencode/opencode.json`
2. Project config: `./opencode.json` or `./.opencode/opencode.jsonc`
3. Environment variables

### Configuration File Format

```json
{
  "$schema": "https://opencode.ai/schema/config.json",
  "theme": "dark",
  "model": "copilot/claude-sonnet-4-5-20250929",
  "small_model": "copilot/claude-3.5-sonnet",
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    },
    "openai": {
      "key": "$OPENAI_API_KEY",
      "base_url": "https://api.openai.com/v1"
    },
    "copilot": {
      "key": "$GITHUB_COPILOT_TOKEN"
    }
  },
  "server": {
    "port": 19876,
    "hostname": "127.0.0.1"
  },
  "tui": {
    "scroll_speed": 3.0
  }
}
```

### Environment Variables

- `ANTHROPIC_API_KEY`: Anthropic API key
- `OPENAI_API_KEY`: OpenAI API key
- `GITHUB_COPILOT_TOKEN`: GitHub Copilot access token
- `OPENCODE_MODEL`: Override default model
- `OPENCODE_THEME`: Override theme (dark/light)
- `OPENCODE_LOG_LEVEL`: Set log level (debug/info/warn/error)

## Available Tools

opencode-rs includes built-in tools that the AI can use:

- **read**: Read file contents
- **write**: Create or overwrite files
- **edit**: Edit files with precise string replacement
- **bash**: Execute shell commands
- **glob**: Find files by pattern
- **grep**: Search file contents
- **todo**: Task list management

## Themes

Themes are located in the `themes/` directory:
- `deltarune.json`: Dark World inspired theme
- `undertale.json`: Underground inspired theme

Custom themes can be added by creating JSON files in the themes directory.

## Project Structure

```
opencode-rs/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── config.rs        # Configuration management
│   ├── provider/        # LLM provider integrations
│   ├── session/         # Session management
│   ├── storage/         # Data persistence
│   ├── tool/            # Built-in tools
│   ├── tui/             # Terminal UI
│   └── cli/             # CLI commands
├── themes/              # Color themes
├── .opencode/           # Example configurations
│   ├── agent/           # Agent examples
│   ├── command/         # Command examples
│   └── opencode.jsonc   # Example config
├── Cargo.toml           # Rust dependencies
└── README.md            # This file
```

## Development

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running with Debug Logging

```bash
RUST_LOG=debug cargo run
```

## Troubleshooting

### "No such device or address" Error

This error occurs when running in non-interactive mode (e.g., piped input). Use the `prompt` command instead:

```bash
# Instead of:
echo "hello" | opencode

# Use:
opencode prompt "hello"
```

### No Model Configured

If you see "No default model configured", make sure to:
1. Initialize config: `opencode config init`
2. Set your API key in the config file
3. Set the `model` field in the config

### API Key Not Found

Make sure your API keys are set either:
- In the config file (can use `$ENV_VAR` syntax)
- As environment variables
- Both methods work together

## Comparison with opencode-ts

opencode-rs aims to be compatible with opencode-ts while providing:
- Faster startup and execution
- Lower memory usage
- Single binary distribution
- Native performance for file operations

Note: Some features from opencode-ts may not be fully implemented yet. This is an ongoing project.

## License

MIT

## Credits

Based on [opencode](https://github.com/anomalyco/opencode) by the Opencode team.
