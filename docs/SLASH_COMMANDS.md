# Slash Commands

This document describes the slash command feature in opencode-rs.

## Overview

Slash commands allow you to execute special commands in the TUI by prefixing them with `/`. They provide quick access to common operations and can be customized via configuration.

## Built-in Commands

### /help [command]
Shows available commands or help for a specific command.

**Examples:**
```
/help
/help clear
```

**Aliases:** `/?`

### /clear
Clears the current session and starts a new one.

**Aliases:** `/new`

### /model [provider/model]
Switches to a different AI model.

**Examples:**
```
/model anthropic/claude-3-5-sonnet-20241022
/model openai/gpt-4
```

### /agent [name]
Switches to a different agent.

**Examples:**
```
/agent general
/agent explore
```

## Custom Commands

You can define custom slash commands in your `opencode.json` or `opencode.jsonc` configuration file.

### Configuration Format

```jsonc
{
  "command": {
    "explain": {
      "template": "Explain $1 in detail with examples",
      "description": "Explain a topic in detail",
      "agent": "general",
      "model": "anthropic/claude-3-5-sonnet-20241022"
    },
    "review": {
      "template": "Review the following code for best practices, bugs, and improvements:\n\n$ARGUMENTS",
      "description": "Review code for quality"
    },
    "translate": {
      "template": "Translate the following from $1 to $2:\n\n$3",
      "description": "Translate text between languages"
    }
  }
}
```

### Template Syntax

Templates support the following placeholders:

- `$1`, `$2`, `$3`, etc. - Positional arguments
- `$ARGUMENTS` - All arguments joined by space
- Last placeholder swallows all remaining arguments

**Examples:**

Template: `"Explain $1 in detail"`
Command: `/explain Rust ownership`
Result: `"Explain Rust ownership in detail"`

Template: `"First: $1, Rest: $2"`
Command: `/cmd one two three four`
Result: `"First: one, Rest: two three four"`

Template: `"All: $ARGUMENTS"`
Command: `/cmd foo bar baz`
Result: `"All: foo bar baz"`

### Command Configuration Options

- `template` (required): The prompt template with placeholders
- `description` (optional): Description of what the command does
- `agent` (optional): Which agent to use for this command
- `model` (optional): Which model to use for this command
- `subtask` (optional): Whether to run as a subtask (not yet implemented)

## Argument Parsing

Arguments support quoted strings for values containing spaces:

```
/cmd "argument with spaces" single
/cmd 'single quoted' "double quoted"
```

## Implementation Details

The slash command system consists of:

- `slash_command/mod.rs` - Core types and traits
- `slash_command/parser.rs` - Command parsing and template expansion
- `slash_command/registry.rs` - Command registration and lookup
- `slash_command/builtin/` - Built-in command implementations
- `slash_command/template.rs` - Template-based custom commands

Commands are registered during app initialization and executed when user input starts with `/`.

## Future Enhancements

Planned improvements include:

- [ ] Autocomplete suggestions while typing commands
- [ ] Command history and recall
- [ ] Subtask support
- [ ] Shell command execution in templates (`` !`command` `` syntax)
- [ ] MCP (Model Context Protocol) integration for external commands
- [ ] Per-command keybindings
- [ ] Command aliases in config
