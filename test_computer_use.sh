#!/bin/bash

echo "Testing computer_use subagent - Describe screen and close Finder"
echo ""

# First test - describe what's on screen
echo "Test 1: Describe the screen"
echo '{"action": "describe_screen"}' | cargo run --release -- tool computer_use

echo ""
echo "Test 2: Navigate to Finder close button"
echo '{"action": "navigate_to", "description": "red close button in the top left of the Finder window"}' | cargo run --release -- tool computer_use

echo ""
echo "Done!"