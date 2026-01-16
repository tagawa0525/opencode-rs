# Usage Guide

## Quick Start

### 1. Build the Project

```bash
cargo build --release
```

The compiled binary will be at `./target/release/opencode`.

### 2. Initialize Configuration

```bash
./target/release/opencode config init
```

This creates `~/.config/opencode/opencode.json` with default settings.

### 3. Configure API Keys

Edit `~/.config/opencode/opencode.json` and add your API keys:

```json
{
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    }
  },
  "model": "anthropic/claude-3-5-sonnet-20241022"
}
```

Set the environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

## Running Modes

### TUI Mode (Main Feature)

**Interactive terminal mode** - the primary way to use opencode:

```bash
./target/release/opencode
```

This launches a full-screen terminal interface where you can:
- Chat with the AI assistant
- See responses in real-time
- Use keyboard shortcuts (Ctrl+C to quit, Enter to submit)

**Requirements:**
- Must run in an actual terminal emulator
- Does NOT work with `cargo run` in some environments
- Does NOT work in pipes or non-TTY contexts

**Keyboard Shortcuts:**
- `Enter`: Submit message
- `Ctrl+C` or `Ctrl+D`: Quit
- `Ctrl+L`: Clear input
- Arrow keys: Navigate cursor
- `Alt+Enter`: Insert newline

### Prompt Mode (Non-Interactive)

**Single-shot mode** for scripts and automation:

```bash
./target/release/opencode prompt "your question here"
```

Options:
- `-m, --model`: Specify model (e.g., `anthropic/claude-3-5-sonnet-20241022`)
- `--format`: Output format (`text`, `json`, `markdown`)

Examples:

```bash
# Simple question
./target/release/opencode prompt "what is Rust?"

# With specific model
./target/release/opencode prompt "explain this code" -m openai/gpt-4

# JSON output
./target/release/opencode prompt "list files" --format json

# Markdown output
./target/release/opencode prompt "explain architecture" --format markdown
```

## Configuration

### Configuration Files

Configuration is loaded in order (later overrides earlier):

1. **Global config**: `~/.config/opencode/opencode.json`
2. **Project config**: `./opencode.json` or `./.opencode/opencode.jsonc`
3. **Environment variables**

### Configuration Commands

```bash
# Show current configuration
./target/release/opencode config show

# Show configuration file paths
./target/release/opencode config path

# Initialize default configuration
./target/release/opencode config init
```

### Configuration Options

#### Provider Configuration

```json
{
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    },
    "openai": {
      "key": "$OPENAI_API_KEY",
      "base_url": "https://api.openai.com/v1"
    }
  }
}
```

#### Model Selection

```json
{
  "model": "anthropic/claude-3-5-sonnet-20241022",
  "small_model": "anthropic/claude-3-haiku-20240307"
}
```

#### TUI Settings

```json
{
  "tui": {
    "scroll_speed": 3.0,
    "diff_style": "auto"
  }
}
```

#### Server Settings

```json
{
  "server": {
    "port": 19876,
    "hostname": "127.0.0.1"
  }
}
```

### Environment Variables

- `ANTHROPIC_API_KEY`: Anthropic API key
- `OPENAI_API_KEY`: OpenAI API key
- `OPENCODE_MODEL`: Override default model
- `OPENCODE_THEME`: Theme (dark/light)
- `OPENCODE_LOG_LEVEL`: Log level (debug/info/warn/error)

You can reference environment variables in config files with `$VAR_NAME`.

## Session Management

```bash
# List all sessions
./target/release/opencode session list

# Show session details
./target/release/opencode session show <session-id>

# Delete a session
./target/release/opencode session delete <session-id>
```

## Common Issues

### TTY Error

**Problem**: "This command requires a TTY (terminal)"

**Solutions**:
1. Run the compiled binary directly in a terminal, not through `cargo run`
2. Use prompt mode instead: `opencode prompt "your message"`
3. Ensure you're in an interactive terminal, not a script or pipe

### No Model Configured

**Problem**: "No default model configured"

**Solutions**:
1. Run `opencode config init`
2. Edit config file to add API key and model
3. Set environment variables

### Model Not Found

**Problem**: "Model not found: provider/model"

**Solutions**:
1. Check model name format: `provider/model`
2. Verify provider is configured with API key
3. Check provider documentation for available models

### API Key Not Found

**Problem**: "No API key for provider"

**Solutions**:
1. Add key to config file: `"key": "$ANTHROPIC_API_KEY"`
2. Set environment variable: `export ANTHROPIC_API_KEY="..."`
3. Verify environment variable is set: `echo $ANTHROPIC_API_KEY`

## Development

### Running Tests

```bash
cargo test
```

### Development Mode

```bash
# Build in debug mode
cargo build

# Run with logging
RUST_LOG=debug cargo run
```

### Building for Production

```bash
# Optimized release build
cargo build --release

# Strip debug symbols (smaller binary)
strip target/release/opencode
```

## Advanced Usage

### Custom Instructions

Add custom instructions in your config:

```json
{
  "instructions": ["STYLE_GUIDE.md", ".opencode/instructions.md"]
}
```

### MCP Servers

Configure Model Context Protocol servers:

```json
{
  "mcp": {
    "my-server": {
      "type": "remote",
      "url": "https://mcp.example.com"
    }
  }
}
```

### Project-Specific Config

Create `.opencode/opencode.jsonc` in your project:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  // Project-specific settings
  "model": "anthropic/claude-3-5-sonnet-20241022",
  "instructions": ["PROJECT_GUIDELINES.md"]
}
```

## Tips

1. **Use environment variables for secrets**: Never commit API keys to git
2. **Start with TUI mode**: It's the best experience for interactive development
3. **Use prompt mode for automation**: Perfect for scripts and CI/CD
4. **Configure per-project**: Add `.opencode/opencode.jsonc` to projects
5. **Check logs**: Use `RUST_LOG=debug` to troubleshoot issues
