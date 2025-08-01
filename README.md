# Assistant

A powerful, modular AI assistant framework built in Rust with advanced computer use capabilities, web search, knowledge synthesis, and tool delegation.

## Features

- **Computer Use Agent**: Take screenshots, control mouse/keyboard, and interact with desktop applications
- **Web Search & Fetch**: Search the web and fetch content from URLs with specialized AI agents
- **Knowledge Synthesis**: Process and synthesize information from multiple sources with memory capabilities
- **Tool Delegation**: Automatically route complex tasks to specialized sub-agents
- **Streaming Responses**: Real-time response streaming with proper completion handling
- **Modular Architecture**: Actor-based system with configurable tools and agents
- **Multiple Model Support**: Works with OpenAI, OpenRouter, and other OpenAI-compatible APIs

## Quick Start

### Prerequisites

- Rust (latest stable)
- macOS (for computer use features)
- `cliclick` installed for mouse/keyboard control:
  ```bash
  brew install cliclick
  ```

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/your-org/assistant.git
   cd assistant
   ```

2. Copy the example config and add your API keys:
   ```bash
   cp config.example.json config.json
   # Edit config.json with your API keys
   ```

3. Build the project:
   ```bash
   cargo build --release
   ```

### Configuration

Edit `config.json` with your API credentials:

```json
{
  "api_key": "your-api-key-here",
  "base_url": "https://openrouter.ai/api/v1",
  "model": "anthropic/claude-3.5-sonnet",
  "tools": {
    "computer_use": {
      "enabled": true,
      "delegate": true,
      "model": "anthropic/claude-3-opus",
      "system_prompt": "You are a computer-use agent..."
    }
  }
}
```

## Usage

### Computer Use

Take screenshots and control your computer:

```bash
# Take a screenshot and describe what's visible
cargo run --release -- --config ./config.json tool computer_use '{"action":"describe_screen"}'

# The agent can also click, type, and perform complex tasks
# Example: "Click on the Safari icon in the dock"
```

### Web Search

Search the web with specialized AI agents:

```bash
# Search for information
cargo run --release -- --config ./config.json tool web_search '{"query":"latest AI developments"}'

# Fetch specific URLs
cargo run --release -- --config ./config.json tool web_fetch '{"url":"https://example.com"}'
```

### Knowledge Synthesis

Process and synthesize information:

```bash
cargo run --release -- --config ./config.json tool knowledge_agent '{"task":"summarize recent papers on AI safety"}'
```

### Interactive Mode

Start an interactive session:

```bash
cargo run --release -- --config ./config.json
```

## Architecture

The assistant uses an actor-based architecture with the following components:

### Core Actors

- **Client Actor**: Handles communication with AI models (OpenAI, OpenRouter, etc.)
- **Chat Actor**: Manages conversation flow and message history
- **Delegator**: Routes tool requests to appropriate handlers
- **Sub-Agent System**: Specialized agents for complex tasks

### Tools

- **Computer Use**: Desktop interaction via screenshots and cliclick
- **Web Search**: Internet search with configurable search engines
- **Web Fetch**: URL content retrieval and processing
- **Knowledge Agent**: Information synthesis and memory
- **File System**: Read, write, and navigate files
- **Memory**: Persistent storage for conversations and knowledge

### Configuration

The system is highly configurable through `config.json`:

- **API Settings**: Choose your AI provider and model
- **Tool Configuration**: Enable/disable tools and set their behavior
- **Sub-Agent Delegation**: Route complex tasks to specialized agents
- **Model Selection**: Use different models for different tasks

## Computer Use Capabilities

The computer use agent can:

### Screenshot & Vision
- Capture full screen or specific areas
- Analyze and describe visual content
- Identify UI elements and their locations

### Mouse Control
- Click at specific coordinates or on identified elements
- Drag and drop operations
- Right-click and context menus
- Scroll and zoom

### Keyboard Input
- Type text and special characters
- Keyboard shortcuts and hotkeys
- Text selection and editing

### Complex Tasks
- Navigate applications and websites
- Fill out forms and interact with UIs
- Automate multi-step workflows
- Respond to visual changes and prompts

## Development

### Running Tests

```bash
cargo test
```

### Adding New Tools

1. Implement the tool in `crates/assistant-core/src/actors/tools/`
2. Register it in the tool registry
3. Add configuration options to the config schema
4. Update the delegator if sub-agent support is needed

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## Safety & Privacy

- **Local Execution**: All computer control happens locally
- **API Communication**: Only sends necessary data to configured AI providers
- **Screenshot Privacy**: Images are processed by your chosen AI model
- **No Telemetry**: Optional telemetry can be disabled in config

## Troubleshooting

### Common Issues

**Tool timeouts**: Increase timeout values in config or check model response times

**Permission errors**: Ensure cliclick has accessibility permissions on macOS

**API errors**: Verify API keys and base URLs in config.json

**Build errors**: Ensure you have the latest Rust toolchain

### Debug Mode

Run with detailed logging:

```bash
RUST_LOG=debug cargo run --release -- --config ./config.json [command]
```

## License

[Add your license information here]

## Acknowledgments

Built with:
- [Tokio](https://tokio.rs/) for async runtime
- [Serde](https://serde.rs/) for serialization
- [Reqwest](https://github.com/seanmonstar/reqwest) for HTTP clients
- [Tracing](https://tracing.rs/) for logging

## Support

For issues and questions:
- Create an issue on GitHub
- Check the troubleshooting section
- Review the configuration examples