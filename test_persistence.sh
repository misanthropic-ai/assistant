#!/bin/bash

echo "Testing chat persistence..."

# Get session ID before running commands
SESSION_ID=$(sqlite3 ~/.assistant/assistant.db "SELECT id FROM sessions ORDER BY created_at DESC LIMIT 1;")
echo "Current session ID: $SESSION_ID"

# Run a simple prompt
echo "Test 1: Simple prompt"
cargo run --release -- prompt "Hello, this is test message 1"

# Run a prompt that uses tools
echo -e "\nTest 2: Tool-using prompt"
cargo run --release -- prompt "Create a test file called persistence_test.txt with the content 'Test successful'"

# Check messages were persisted
echo -e "\nChecking persisted messages..."
sqlite3 ~/.assistant/assistant.db "SELECT role, substr(content, 1, 80) FROM chat_messages WHERE session_id = '$SESSION_ID' ORDER BY created_at;"

# Clean up
rm -f persistence_test.txt

echo -e "\nPersistence test complete!"