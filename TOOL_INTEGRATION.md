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

2. **Streaming Events** (`src/provider/streaming.rs`)
   - ✅ Added `ToolResult` event type for tool execution results
   - ✅ Existing tool call events (ToolCallStart, ToolCallDelta, ToolCallEnd)

3. **Tool Executor** (`src/tool/executor.rs`)
   - ✅ `execute_tool()` - executes a single tool
   - ✅ `execute_all_tools()` - batch executes multiple tool calls
   - ✅ `ToolCallTracker` - tracks tool calls during streaming
   - ✅ `build_tool_result_message()` - formats results for LLM

4. **CLI Agentic Loop** (`src/cli/prompt.rs`)
   - ✅ Complete agentic loop implementation
   - ✅ Tool execution and result handling
   - ✅ Conversation history management
   - ✅ Multi-step processing with tool calls

### ⏳ Pending

1. **TUI Integration** (`src/tui/app.rs`)
   - ⏳ Tool execution in interactive mode
   - ⏳ Display tool results in TUI
   - ⏳ Agentic loop for TUI

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
# Ask the LLM to read a file
cargo run -- prompt "Read the file /tmp/test.txt and tell me what it says"

# The flow will be:
# 1. LLM requests read tool
# 2. Tool executes and reads file
# 3. Result sent back to LLM
# 4. LLM responds with file content
```

### Code Example

```rust
use opencode::tool::{ToolCallTracker, ToolContext, execute_all_tools};

// During streaming
let mut tracker = ToolCallTracker::new();

// On ToolCallStart event
tracker.start_call(id, name);

// On ToolCallDelta event
tracker.add_arguments(&id, &delta);

// After stream ends
let pending_calls = tracker.get_all_calls();

if !pending_calls.is_empty() {
    let ctx = ToolContext::new("session", "msg", "agent");
    let results = execute_all_tools(pending_calls, &ctx).await;
    
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

### Differences
1. **No AI SDK Integration**: Rust version manually handles streaming and tool protocol
2. **Simpler State Management**: No complex session/message storage (yet)
3. **Synchronous Tool Execution**: Tools execute sequentially (can be parallelized)
4. **Basic Error Handling**: Simpler error propagation vs TS hooks

## Next Steps

1. **TUI Integration**
   - Adapt agentic loop for TUI event handling
   - Display tool execution progress
   - Handle user interrupts

2. **Enhancements**
   - Parallel tool execution
   - Tool execution timeouts
   - Permission system integration
   - Doom loop detection (repetitive tool calls)

3. **Testing**
   - Unit tests for tool executor
   - Integration tests for agentic loop
   - Test with various LLM providers

## References

- TypeScript implementation: `opencode-ts/packages/opencode/src/session/prompt.ts`
- Tool protocol: Anthropic Messages API documentation
- Original issue: Tools were defined but not executed
