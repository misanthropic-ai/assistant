# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Structure

This is an actor-based AI assistant built in Rust using the `ractor` framework. The project implements a port of qwen-code with enhanced architecture for concurrency and tool delegation.

```
assistant/
├── Cargo.toml           # Workspace configuration
├── config.json          # Runtime configuration (API keys, models, tools)
├── crates/
│   ├── assistant-core/  # Core actor system and tools
│   │   └── src/
│   │       ├── actors/      # Actor implementations
│   │       ├── config/      # Configuration with tool delegation
│   │       ├── messages.rs  # Actor message types
│   │       └── lib.rs       # Core initialization
│   └── assistant-cli/   # CLI interface
│       └── src/
│           └── main.rs      # Entry point
```

## Build and Development Commands

### Rust Project
- Build: `cargo build`
- Run: `cargo run`
- Build release: `cargo build --release`
- Run release: `cargo run --release`
- Test: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy`
- Check: `cargo check`

### Adding Dependencies
Always use `cargo add` to ensure latest versions:
```bash
cd crates/assistant-core && cargo add <dependency>
cd crates/assistant-cli && cargo add <dependency>
```

## Actor Architecture

The system uses an Erlang-like actor model with `ractor`:

1. **SupervisorActor** - Root actor managing the system
2. **ChatActor** - Manages conversation flow and history
3. **ClientActor** - Handles OpenAI API communication
4. **DelegatorActor** - Routes tools to specialized LLMs
5. **Tool Actors**:
   - FileSystemActor (ls, read, write, edit, glob, grep)
   - ShellActor (command execution)
   - WebActor (fetch, search)
   - MemoryActor (context management)

## Configuration

The `config.json` file supports:
- Primary LLM configuration (api_key, model, temperature)
- Tool-specific configurations with delegation
- Blacklist approach for tool exclusion

Example with tool delegation:
```json
{
  "tools": {
    "web_search": {
      "delegate": true,
      "api_key": "sk-different-key",
      "model": "gpt-4-vision-preview",
      "system_prompt": "You are a web search specialist..."
    }
  }
}
```

## Key Implementation Details

### Actor Communication
- All actors communicate via typed messages
- Use `ActorRef<MessageType>` for sending messages
- Actors have separate State and Arguments types

### Tool Implementation
- All tools implement `ToolActorTrait`
- Tools can be delegated to specialized LLMs
- Tool execution is concurrent by default

### Error Handling
- Use `anyhow::Result` for general errors
- Actor errors use `ActorProcessingErr`
- Tools return structured `ToolResult`

## Future Extensions

The architecture is designed for:
- Parallel subagents (child actors)
- Hot code reload via actor restart
- Distributed execution across machines
- Tool-specific fine-tuned models

## Development Tips

1. When adding a new tool:
   - Create actor in `actors/tools/`
   - Implement `ToolActorTrait`
   - Add to tool registry
   - Update configuration types

2. For actor communication:
   - Define messages in `messages.rs`
   - Keep messages small and serializable
   - Use UUID for request tracking

3. Testing actors:
   - Use `Actor::spawn` in tests
   - Send messages and await responses
   - Test error cases and timeouts