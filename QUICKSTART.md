# Quick Start Guide

Get started with opencode-rs in 3 simple steps!

## Step 1: Build

```bash
cd opencode-rs
cargo build --release
```

Wait for the build to complete. The binary will be at `./target/release/opencode`.

## Step 2: Configure

### Initialize Config

```bash
./target/release/opencode config init
```

### Add Your API Key

Edit the config file at `~/.config/opencode/opencode.json`:

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

### Set Environment Variable

```bash
export ANTHROPIC_API_KEY="sk-ant-api03-..."
```

**Tip**: Add this to your `~/.bashrc` or `~/.zshrc` to make it permanent.

## Step 3: Run!

### TUI Mode (Interactive - Recommended!)

Open a **real terminal** and run:

```bash
./target/release/opencode
```

You should see a full-screen interface. Type your message and press Enter!

**Important**: If you get a TTY error, make sure you're running the binary directly in a terminal, NOT through `cargo run`.

### Prompt Mode (Non-Interactive)

For quick questions without the TUI:

```bash
./target/release/opencode prompt "Hello! Can you help me with Rust?"
```

## That's It! 🎉

You're ready to use opencode-rs. Check out the [full documentation](README.md) for more features.

## Common First-Time Issues

### "TTY Error" when running

**Problem**: Running through `cargo run` or in a non-terminal environment

**Solution**: Run the compiled binary directly in a real terminal:
```bash
./target/release/opencode  # Not: cargo run
```

### "No model configured"

**Problem**: API key not set

**Solution**: 
1. Make sure you ran `opencode config init`
2. Edit the config file to add your API key
3. Set the environment variable

### "Model not found"

**Problem**: Incorrect model name

**Solution**: Use the full format: `provider/model`
- Example: `anthropic/claude-3-5-sonnet-20241022`
- Example: `openai/gpt-4`

## Next Steps

- Read the [Usage Guide](USAGE.md) for detailed features
- Check [STATUS.md](STATUS.md) for current limitations
- See [README.md](README.md) for comprehensive documentation

## Need Help?

If you encounter issues:

1. Check if you're in a real terminal (not a pipe or script)
2. Verify your API key is set: `echo $ANTHROPIC_API_KEY`
3. Run with debug logging: `RUST_LOG=debug ./target/release/opencode`
4. Read the error message carefully - they're designed to be helpful!

Happy coding! 🦀
