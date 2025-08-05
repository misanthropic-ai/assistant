# Assistant TUI

The Terminal User Interface (TUI) for the assistant has been successfully implemented!

## Features

- **Interactive Chat Interface**: Full-screen terminal interface with message history
- **ASCII Art Header**: Responsive logo that adapts to terminal width
- **Message Types**: Different formatting for user, assistant, tool, error, and info messages
- **Real-time Input**: Type messages with cursor navigation and history support
- **Keyboard Shortcuts**:
  - `Ctrl+C`: Exit the application
  - `Ctrl+L`: Clear the screen
  - `Ctrl+O`: Toggle error details
  - `Tab`: Autocomplete (coming soon)
  - `↑/↓`: Navigate through message history
  - `Page Up/Down`: Scroll through chat history

## Running the TUI

To launch the interactive TUI, run:

```bash
cargo run -p assistant-cli -- interactive
```

## Architecture

The TUI is built using:
- **ratatui**: Modern terminal UI framework for Rust
- **crossterm**: Cross-platform terminal manipulation
- **Actor-based architecture**: Integrates with the existing actor system

## Layout

```
┌─────────────────────────────────────────┐
│    ___         _     __            __   │
│   / _ | ___ __(_)__ / /____ ____  / /  │  <- Header with ASCII art
│  / __ |(_-<(_-< (_-</ __/ _ `/ _ \/ __/ │
│ /_/ |_/___/___/___/\__/\_,_/_//_/\__/  │
├─────────────────────────────────────────┤
│ ┌─────────────────────────────────────┐ │
│ │ Chat                                │ │
│ │                                     │ │  <- Message history area
│ │ [09:42:15] Info:                    │ │
│ │ Welcome to Assistant TUI! Type your │ │
│ │ message and press Enter to chat.    │ │
│ │                                     │ │
│ │ [09:42:30] You: Hello!              │ │
│ │                                     │ │
│ │ [09:42:31] Assistant: Hi there! How │ │
│ │ can I help you today?               │ │
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ Type your message or @file          │ │  <- Input area
│ │ ❯ _                                 │ │
│ └─────────────────────────────────────┘ │
├─────────────────────────────────────────┤
│ Model: default | Ctrl+C: Exit | Ctrl... │  <- Footer with shortcuts
└─────────────────────────────────────────┘
```

## Next Steps

- Connect to the actual chat actor system for real AI responses
- Implement command autocomplete
- Add syntax highlighting for code blocks
- Add theme selection dialog
- Implement file browser for @ mentions