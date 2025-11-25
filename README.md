# chat.sacha.house

Single global chat room with no authentication. Pick a username, chat in real-time.

## Stack

- **Backend**: Rust + warp + SQLite
- **Frontend**: Svelte 5 + Bun + Vite
- **Real-time**: Server-Sent Events (SSE)

## Build

```bash
make release  # Build frontend + release binary
make dev     # Build frontend + run dev server
```

## Run

```bash
cargo run
```

Server starts on `http://127.0.0.1:3030` (configurable via `BIND_HOST` and `BIND_PORT` env vars).

## Docker

```bash
docker build -t chat-sacha-house .
docker run -p 3030:3030 chat-sacha-house
```

## Features

- Username reservation via session cookie
- Real-time message delivery via SSE
- Infinite message history in SQLite
- Rate limiting (1 msg/sec per session)
- Auto-release username after 60s inactivity
