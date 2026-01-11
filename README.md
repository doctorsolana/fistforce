# 3D Sandbox Game

A multiplayer open-world sandbox game built with Rust and Bevy.

## Architecture

- **Server**: Headless authoritative server that manages game state
- **Client**: Renders the world and handles player input
- **Shared**: Common code (components, protocol, constants)

## Requirements

- Rust (latest stable)
- macOS, Windows, or Linux

## Quick Start

The easiest way to run the game:

```bash
./run.sh
```

This starts the server in the background, then launches the client.

### Run Script Options

```bash
./run.sh          # Start both server and client (default)
./run.sh both     # Same as above
./run.sh server   # Start only the server
./run.sh client   # Start only the client
```

### Manual Start

If you prefer to run things manually:

```bash
# Terminal 1 - Server
cargo run -p server --release

# Terminal 2 - Client
cargo run -p client --release
```

## Controls

- **WASD**: Move
- **Mouse**: Look around
- **Click**: Grab cursor for FPS mode
- **Escape**: Release cursor

## Development

First build may take a few minutes to compile dependencies.

For faster iteration during development, you can enable dynamic linking:

```bash
cargo run -p client --features bevy/dynamic_linking
```

## Tech Stack

- [Bevy](https://bevyengine.org/) - Game engine
- [Lightyear](https://github.com/cBournhonesque/lightyear) - Networking
