#!/usr/bin/env bash
# Test script to verify TTY detection

echo "=== Testing TTY Detection ==="
echo ""

echo "1. Testing if stdout is a TTY:"
if [ -t 1 ]; then
    echo "   ✓ stdout is a TTY (interactive terminal)"
else
    echo "   ✗ stdout is NOT a TTY"
fi

echo ""
echo "2. Testing opencode TTY check:"
cargo build --release 2>&1 | grep -q "Finished" && echo "   ✓ Build successful"

echo ""
echo "3. Running opencode (should fail with TTY error in non-TTY environment):"
./target/release/opencode 2>&1 | head -3

echo ""
echo "4. Testing prompt command (should work without TTY):"
./target/release/opencode prompt --help | head -5

echo ""
echo "=== To test TUI mode in a real terminal ==="
echo "Run: ./target/release/opencode"
echo "This should work if you're in an interactive terminal."
