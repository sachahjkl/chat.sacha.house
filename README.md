# chat.sacha.house

Single global chat room with no authentication. Pick a username, chat in real-time.

## Stack

- **Backend**: Rust + warp + SQLite
- **Frontend**: Svelte 5 + Bun + Vite
- **Real-time**: Server-Sent Events (SSE)

## Build

```bash
nix build
```

## Run

```bash
cargo run
```

Server starts on `http://127.0.0.1:3030` (configurable via `BIND_HOST` and `BIND_PORT` env vars).

Environment variables:
- `BIND_HOST`: Server bind address (default: `127.0.0.1`)
- `BIND_PORT`: Server port (default: `3030`)
- `RATE_LIMIT_SECS`: Rate limit window in seconds for message sending (default: `1`)

## Container

```bash
nix build .#dockerImage
podman load < result
podman run --rm -p 3030:3030 chat-sacha-house:0.1.0
```

## Features

- Username reservation via session cookie
- Real-time message delivery via SSE
- Infinite message history in SQLite
- Rate limiting (1 msg/sec per session)
- Auto-release username after 60s inactivity
