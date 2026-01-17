# Tool Integration Implementation

This document describes the implementation of the agentic loop and tool execution in opencode-rs, based on the TypeScript reference implementation.

## Overview

The tool execution system enables the LLM to call tools (like `read`, `write`, `bash`, etc.) and receive the results, creating an agentic loop where the LLM can continue working until the task is complete.

## Implementation Status

### ✅ Completed

1. **Tool Infrastructure** (`src/tool/`)
   - ✅ Tool definitions and registry
   - ✅ Individual tool implementations (read, write, edit, bash, glob, grep)
   - ✅ Tool execution framework
   - ✅ Parallel tool execution
   - ✅ Doom loop detection

2. **Streaming Events** (`src/provider/streaming.rs`)
   - ✅ Added `ToolResult` event type for tool execution results
   - ✅ Existing tool call events (ToolCallStart, ToolCallDelta, ToolCallEnd)

3. **Tool Executor** (`src/tool/executor.rs`)
   - ✅ `execute_tool()` - executes a single tool
   - ✅ `execute_all_tools()` - sequential execution
   - ✅ `execute_all_tools_parallel()` - parallel execution (NEW!)
   - ✅ `ToolCallTracker` - tracks tool calls during streaming
   - ✅ `DoomLoopDetector` - detects repetitive tool calls (NEW!)
   - ✅ `build_tool_result_message()` - formats results for LLM

4. **CLI Agentic Loop** (`src/cli/prompt.rs`)
   - ✅ Complete agentic loop implementation
   - ✅ Tool execution and result handling
   - ✅ Conversation history management
   - ✅ Multi-step processing with tool calls
   - ✅ Doom loop detection and warnings (NEW!)
   - ✅ Permission checking (NEW!)

5. **Permission System** (`src/permission.rs`) (NEW!)
   - ✅ `PermissionChecker` - checks tool permissions
   - ✅ CLI permission prompts
   - ✅ Config-based permission rules
   - ✅ Doom loop permission handling

### ⏳ Future Enhancements

1. **TUI Integration** (`src/tui/app.rs`)
   - ⏳ Full agentic loop in interactive mode
   - ⏳ Display tool results in TUI
   - ⏳ Interactive permission prompts in TUI

2. **Advanced Features**
   - ⏳ Tool execution timeouts
   - ⏳ Tool dependency resolution
   - ⏳ Tool result caching

## Architecture

### Agentic Loop Flow

```
User Message
    ↓
┌─────────────────────┐
│  LLM Streaming      │
│  (with tools)       │
└─────────────────────┘
    ↓
┌─────────────────────┐
│ Collect Events:     │
│ - TextDelta         │
│ - ToolCallStart     │
│ - ToolCallDelta     │
│ - ToolCallEnd       │
└─────────────────────┘
    ↓
  Has Tool Calls? ──No──> Done
    │
   Yes
    ↓
┌─────────────────────┐
│ Execute Tools       │
│ (in parallel)       │
└─────────────────────┘
    ↓
┌─────────────────────┐
│ Add Tool Results    │
│ to Conversation     │
└─────────────────────┘
    ↓
  Loop back to LLM
```

### Key Components

#### 1. ToolCallTracker

Tracks tool calls as they stream from the LLM:

```rust
pub struct ToolCallTracker {
    calls: HashMap<String, (String, String)>, // id -> (name, arguments)
}
```

Methods:
- `start_call()` - Register new tool call
- `add_arguments()` - Append argument deltas
- `finish_call()` - Finalize and return tool call
- `get_all_calls()` - Get all pending calls

#### 2. Tool Execution

```rust
pub async fn execute_tool(
    tool_name: &str,
    arguments: &str,
    _tool_id: &str,
    ctx: &ToolContext,
) -> Result<ToolResult>
```

Executes a single tool by:
1. Parsing arguments JSON
2. Looking up tool in registry
3. Executing with ToolContext
4. Returning ToolResult

#### 3. Conversation Format

The conversation follows Anthropic's tool calling format:

**User Message:**
```json
{
  "role": "user",
  "content": "Read the file /tmp/test.txt"
}
```

**Assistant with Tool Call:**
```json
{
  "role": "assistant",
  "content": [
    {
      "type": "tool_use",
      "id": "tool_1",
      "name": "read",
      "input": {"filePath": "/tmp/test.txt"}
    }
  ]
}
```

**User with Tool Result:**
```json
{
  "role": "user",
  "content": [
    {
      "type": "tool_result",
      "tool_use_id": "tool_1",
      "content": "{\"title\":\"Read file\",\"output\":\"Hello World\"}",
      "is_error": false
    }
  ]
}
```

**Final Assistant Response:**
```json
{
  "role": "assistant",
  "content": "The file contains 'Hello World'"
}
```

## Usage

### CLI Example

```bash
# Basic usage - tools run with permission checks
cargo run -- prompt "Read the file Cargo.toml and tell me what it says"

# The flow will be:
# 1. LLM requests read tool (permission: allow by default)
# 2. Tool executes and reads file
# 3. Result sent back to LLM
# 4. LLM responds with file content

# Example with write tool (requires permission)
cargo run -- prompt "Create a new file called test.txt with 'Hello World'"

# The flow will be:
# 1. LLM requests write tool
# 2. System asks for permission (write requires confirmation)
# 3. User approves
# 4. Tool executes
# 5. LLM confirms success
```

### Permission Configuration

Create an `opencode.json` file to customize permissions:

```json
{
  "permission": {
    "read": "allow",    // Always allow
    "write": "ask",     // Ask for confirmation
    "edit": "ask",      // Ask for confirmation
    "bash": "deny",     // Never allow
    "doom_loop": "allow" // Don't ask about doom loops
  }
}
```

### Code Example

```rust
use opencode::tool::{ToolCallTracker, DoomLoopDetector, ToolContext, execute_all_tools_parallel};
use opencode::permission::PermissionChecker;

// Create checkers
let mut tracker = ToolCallTracker::new();
let mut doom_detector = DoomLoopDetector::new();
let permission_checker = PermissionChecker::from_config(&config);

// During streaming
tracker.start_call(id, name);
tracker.add_arguments(&id, &delta);

// After stream ends
let pending_calls = tracker.get_all_calls();

if !pending_calls.is_empty() {
    // Check for doom loop
    doom_detector.add_calls(&pending_calls);
    if let Some((tool, args)) = doom_detector.check_doom_loop() {
        // Handle doom loop detection
        let allowed = permission_checker
            .check_doom_loop_and_ask_cli(&tool, &args)
            .await?;
        if !allowed {
            return; // Stop execution
        }
    }
    
    // Check permissions for each tool
    let mut approved = Vec::new();
    for call in pending_calls {
        let allowed = permission_checker
            .check_and_ask_cli(&call.name, &call.arguments)
            .await?;
        if allowed {
            approved.push(call);
        }
    }
    
    // Execute approved tools in parallel
    let ctx = ToolContext::new("session", "msg", "agent");
    let results = execute_all_tools_parallel(approved, &ctx).await;
    
    // Add results to conversation and continue
    messages.push(build_tool_result_message(results));
}
```

## Differences from TypeScript Implementation

### Similarities
- Same tool calling protocol (Anthropic format)
- Same conversation structure
- Same tool execution flow
- Same agentic loop concept
- Same doom loop detection threshold (3 identical calls)
- Same permission system architecture

### Differences
1. **No AI SDK Integration**: Rust version manually handles streaming and tool protocol
2. **Simpler State Management**: No complex session/message storage (yet)
3. **Parallel Tool Execution**: ✅ Rust version supports parallel execution natively
4. **CLI-only Permissions**: Permissions implemented for CLI, TUI integration pending
5. **Synchronous Permission Checks**: No async permission UI in TUI yet

## Key Features

### 1. Doom Loop Detection

Automatically detects when the LLM calls the same tool with identical arguments 3 times in a row:

```
[WARNING: Doom loop detected!]
[The LLM has called 'read' with identical arguments 3 times in a row]
[Arguments: {"filePath":"/tmp/test.txt"}]
[This may indicate the LLM is stuck.]
```

### 2. Permission System

Three permission levels:
- `allow` - Execute without asking
- `ask` - Prompt user for confirmation
- `deny` - Never execute

Default permissions:
- `read`, `glob`, `grep` → `allow`
- `write`, `edit`, `bash` → `ask`
- `doom_loop` → `ask`

### 3. Parallel Tool Execution

Multiple tools execute simultaneously for better performance:

```rust
// Sequential (old)
execute_all_tools(calls, &ctx).await

// Parallel (new)
execute_all_tools_parallel(calls, &ctx).await
```

## Next Steps

1. **TUI Integration**
   - Full agentic loop in interactive mode
   - Interactive permission prompts
   - Tool execution progress display

2. **Testing**
   - Unit tests for doom loop detector
   - Integration tests for permission system
   - Test with various LLM providers

3. **Advanced Features**
   - Tool execution timeouts
   - Tool result caching
   - Tool dependency resolution

## References

- TypeScript implementation: `opencode-ts/packages/opencode/src/session/prompt.ts`
- Doom loop detection: `opencode-ts/packages/opencode/src/session/processor.ts`
- Tool protocol: Anthropic Messages API documentation
- Original issue: Tools were defined but not executed
