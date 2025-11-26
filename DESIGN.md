# Chat Room Design Document

## Overview

Single global chat room web application with no authentication. Users pick a username which is reserved to their session via an HttpOnly cookie that contains a JULID. The reservation automatically expires after `SESSION_TIMEOUT_SECONDS` (currently 60 s) of inactivity and can be manually released from the UI. Real-time message delivery uses Server-Sent Events (SSE) backed by a Tokio broadcast channel. The entire system ships as one Rust binary that embeds the Svelte SPA build.

## Core Requirements

- **No Authentication**: Usernames only, no passwords or accounts
- **Session-Based Username Reservation**: Username locked to an HttpOnly cookie containing a JULID
- **Real-Time Updates**: SSE for live message streaming plus REST fallback for history
- **Infinite Message History**: All messages persisted in SQLite, newest-first pagination
- **Self-Contained**: Single executable with embedded Svelte frontend assets
- **Basic Anti-Spam**: In-memory per-session rate limit (1 message/sec)
- **Simple Architecture**: Embrace grug constraints; minimize moving parts

## Architecture

### Backend (Rust + warp)

- **Web Framework**: warp for HTTP server and routing
- **Database**: SQLite with best practices (WAL mode, foreign keys, busy timeout)
- **Real-Time**: tokio::sync::broadcast queue for pub/sub message distribution
- **Message Ordering**: UUIDv7 for timestamp-based sortable IDs
- **Rate Limiting**: In-memory per-session limiter (HashMap + Instant) enforcing ≥1 s between posts
- **Session Management**: HttpOnly, Secure cookies holding JULID session IDs with 60 s inactivity timeout

### Frontend (Svelte)

- **Framework**: Svelte 5 SPA (runed state) living in `front/`
- **Build**: Bun + Vite + Svelte; dist embedded via include_dir
- **Real-Time**: EventSource API talking to `/api/messages/sse`
- **History Rendering**: Simple list view kept sorted newest-first via JULID comparisons
- **XSS Protection**: Render message text as plain text only

## Database Schema

### sessions table

```sql
CREATE TABLE sessions (
  session_id BLOB PRIMARY KEY,        -- JULID bytes
  username TEXT NOT NULL UNIQUE,      -- Unique username
  last_locked INTEGER NOT NULL DEFAULT 0 -- Unix timestamp seconds of most recent lock
) STRICT;

CREATE INDEX idx_sessions_username ON sessions(username);
```

### messages table

```sql
CREATE TABLE messages (
  id BLOB PRIMARY KEY,                -- JULID (sortable, convertible to UUIDv7 string)
  username TEXT NOT NULL,             -- Foreign key to sessions
  text TEXT NOT NULL,                 -- Message content
  created_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY(username) REFERENCES sessions(username)
) STRICT;

CREATE INDEX idx_messages_created ON messages(created_at);
```

### migrations table

```sql
CREATE TABLE migrations (
  version INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  applied_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

Tracks which schema migrations ran so each versioned function executes exactly once.

## API Endpoints

### POST /api/username/claim

Claim a username and establish session.

**Request Body**: `{"username": "string"}`

**Response**: `{"success": true, "session_id": "string", "expires_in": 60}`

**Side Effects**:

- Validates username availability
- Generates session_id (JULID, UUIDv7-compatible)
- Sets cookie: `session_id=<julid>; HttpOnly; Secure; SameSite=Strict; Path=/`
- Inserts/updates row with `last_locked = now`
- Returns remaining TTL so the frontend countdown can display auto-release timing

### POST /api/username/release

Manually release username and end session.

**Response**: `{"success": true}`

**Side Effects**:

- Sets `last_locked = 0` for current session
- Clears session cookie (Max-Age=0)
- Drops rate-limit entry for the released session_id
- Closes SSE stream client-side

### POST /api/messages

Send a message to the chat room.

**Request Body**: `{"text": "string"}`

**Response**: `{"success": true, "message_id": "string"}`

**Flow**:

1. Validate session cookie exists and session has not hit the TTL (`last_locked + timeout > now`)
2. Enforce per-session rate limit (one message per second)
3. Validate message: `length(trimmed(text)) <= 240` and non-empty
4. Generate JULID/UUIDv7 for message ID
5. Insert message into DB and update session `last_locked`
6. Broadcast serialized message JSON to all SSE clients via the broadcast queue

### GET /api/messages

Retrieve message history with pagination.

**Query Parameters**:

- `from`: UUIDv7 message ID (optional, for pagination)
- `limit`: number (default 50, max 100)

**Response**: `{"messages": [{id, username, text, created_at}, ...]}`

Returns messages ordered by UUIDv7 ID descending (newest first).

### GET /api/messages/sse

Server-Sent Events stream for real-time messages.

**Headers**: Cookie with session_id required

**Event Format**:

```
event: message
data: {"id":"string","username":"string","text":"string","created_at":1234567890}
```

**Keep-Alive**: Relies on EventSource automatic reconnection; no explicit ping events.

**Disconnection Handling**: Session remains reserved until `SESSION_TIMEOUT_SECONDS` elapses without activity (determined by `last_locked`).

### GET /api/stats

Get global statistics.

**Response**: `{"total_messages": 12345}`

### GET /

Serve embedded Svelte application (index.html and assets).

## Database Migration Strategy

- On startup the backend enforces SQLite pragmas (WAL, busy timeout, foreign keys) and then runs a small migration harness.
- A `migrations` table records applied versions; each numbered function runs only once and inserts a `(version, name)` row on success.
- Current migrations:
  1. `migrate_create_tables` – creates the original `sessions`/`messages` tables (with `locked` + `last_activity`) and supporting indexes.
  2. `migrate_sessions_last_locked` – rebuilds the `sessions` table into the new `last_locked` shape and recreates indexes with foreign keys temporarily disabled to avoid FK references to the transitional table.
- After all migrations complete, the server asserts that `sessions` exposes only the `last_locked` column to prevent running against a stale schema.

## User Flow

### Initial Connection

1. User visits `/`
2. Frontend prompts for username and POSTs to `/api/username/claim`
3. Server validates availability, generates JULID, sets cookie, returns TTL
4. Frontend immediately fetches `/api/messages` for the latest history
5. Frontend establishes EventSource to `/api/messages/sse`
6. Countdown indicator starts and reflects auto-release timing

### Sending Messages

1. User types message (max 240 chars)
2. Client-side validation (trimmed length, non-empty)
3. POST to `/api/messages`
4. Server enforces rate-limit, validates, inserts to DB, updates session `last_locked`
5. Successful POST resets the client countdown and clears any error status
6. Server broadcasts to all SSE connections; every client inserts the message in sorted order

### Receiving Messages

1. EventSource connection receives `message` events
2. Parse JSON data
3. Insert into sorted list (newest-first) using JULID lexicographic order
4. Handle `RecvError::Lagged` by re-fetching from `/api/messages`

### Scrolling History

1. User taps Refresh or requests pagination (manual for now)
2. GET `/api/messages?from=<oldest_visible_id>&limit=50`
3. Merge results into the sorted message list

### Manual Logout

1. User clicks logout button
2. POST to `/api/username/release`
3. Server sets `last_locked = 0`, clears cookie
4. Frontend closes EventSource and returns to username prompt

### Automatic Session Timeout

1. Countdown starts at `expires_in` seconds received from the claim response
2. Each successful message resets the countdown and bumps `last_locked`
3. When countdown reaches zero without activity, the frontend releases the username and closes SSE
4. Subsequent claim attempts treat the stale session as expired because `last_locked + SESSION_TIMEOUT_SECONDS < now`

## Message Validation

### Server-Side (Enforced)

- Message text after trim must be <= 240 characters
- Message text after trim must be non-empty
- Session cookie must be valid
- Session must still be within the TTL window (`last_locked` freshness)

### Client-Side (UX)

- Same validation as server
- Real-time character counter
- Disable send button if invalid

## Security Considerations

### Session Management

- **HttpOnly**: Prevents JavaScript access to session cookie
- **Secure**: Cookie only sent over HTTPS in production
- **SameSite=Strict**: CSRF protection
- **Session IDs**: JULID (UUIDv7-compatible, cryptographically strong)
- **Expiry**: `SESSION_TIMEOUT_SECONDS` enforced via `last_locked`

### XSS Protection

- Frontend MUST use `textContent` to render messages
- NEVER use `innerHTML` or `v-html` or similar
- Message content treated as plain text only

### Rate Limiting

- Simple in-memory HashMap keyed by JULID string, tracking the last send `Instant`
- Enforced inside `handle_post_message` before inserts
- Limit is currently 1 message per second per session

### Input Validation

- Message length strictly enforced server-side
- Username availability atomically checked in DB
- Foreign key constraints ensure referential integrity

### Abuse Considerations

- No moderation system (intentional design choice)
- No content filtering (public chat, use at own risk)
- Username squatting possible (also intentional - simple design)

## Technical Decisions & Rationale

### Why Session Cookies Instead of IP Locking?

**Problem with IP-based locking**:

- NAT/proxies: Multiple users behind same IP → only one username
- Dynamic IPs: Mobile, DHCP, VPN changes → lose username
- IPv6 privacy extensions: Automatic IP rotation

**Solution**: Session tokens in HttpOnly cookies

- Unique per browser/tab
- Survives IP changes
- Works with shared NATs
- Industry standard approach

### Why DB-First Then Broadcast?

**Alternative**: Broadcast first, save async
**Problem**: If DB save fails, clients see message that disappears on refresh

**Solution**: Save to DB first, then broadcast

- Ensures persistence before visibility
- Messages in DB are source of truth
- Slight latency acceptable for correctness

### Why UUIDv7 for Message IDs?

- Timestamp-based ordering (microsecond precision)
- Globally unique (no collision risk)
- Sortable without additional timestamp column
- Efficient indexing
- Same microsecond ordering undefined, but acceptable (rare)

### Why SQLite Best Practices Matter

Per [Go + SQLite Best Practices](https://jacob.gold/posts/go-sqlite-best-practices/):

- **WAL Mode**: Readers don't block writers, high concurrency
- **Foreign Keys**: Enforce referential integrity
- **Busy Timeout**: Retry on contention instead of immediate error
- **STRICT Tables**: Better type safety in SQLite
- **Bundled SQLite**: Self-contained, no external dependencies

### Why Broadcast Queue Capacity Limits OK?

`tokio::sync::broadcast` has fixed capacity. Slow SSE clients can lag behind.

**Handling**:

- Client detects `RecvError::Lagged`
- Re-fetch missed messages from DB via `/api/messages`
- Trade-off: Simplicity over guaranteed real-time delivery
- Most clients will keep up; slow clients have fallback

### Why 10s Reconnection Grace Period?

- Network blips common (wifi handoff, mobile switching)
- EventSource auto-reconnects by default
- 10s allows reconnection without losing username
- Prevents annoying username conflicts on brief disconnects

### Why 240 Character Limit?

- Similar to Twitter's original limit (familiar UX)
- Encourages concise communication
- Reduces storage and bandwidth
- Easy to validate client and server-side

## Non-Goals / Intentional Limitations

- **No Moderation**: Public, unfiltered chat
- **No Private Messages**: Single global room only
- **No User Profiles**: Just username, nothing else
- **No Message Editing**: Append-only history
- **No Message Deletion**: Permanent record
- **No Read Receipts**: No tracking who read what
- **No Typing Indicators**: Complexity vs value trade-off
- **Username Squatting OK**: Simple is better than complex anti-squat logic
- **Messages Lost on Graceful Shutdown**: Acceptable edge case

## Dependencies

```toml
[dependencies]
warp = { version = "0.4.2", features = ["server", "test"] }
tokio = { version = "1.48.0", features = ["full"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.145"
include_dir = "0.7.4"
futures-util = "0.3.31"
warp-rate-limit = "0.3.0"
anyhow = "1.0.100"
thiserror = "2.0.17"
sqlx = { version = "0.8.2", features = ["runtime-tokio", "sqlite", "macros"] }
julid-rs = { version = "1.6.1803398874989", features = ["serde", "sqlx"] }
log = "0.4.28"
async-stream = "0.3.6"
mime_guess = "2.0.5"
pretty_env_logger = "0.5.0"
libsqlite3-sys = "0.30"
```

## Implementation Phases

1. **SQLite Setup**: DB initialization, migrations, best practices config
2. **Username Management**: Claim/release endpoints with session cookies
3. **Message Broadcast Queue**: tokio::broadcast setup
4. **Message Posting**: POST endpoint with DB-first, then broadcast
5. **SSE Endpoint**: EventSource stream with keep-alive and timeout handling
6. **Message History**: Pagination endpoint with UUIDv7 ordering
7. **Stats Endpoint**: Message counter
8. **Rate Limiting**: Middleware integration
9. **Frontend Embedding**: include_dir for Svelte dist serving

## Future Considerations (Out of Scope for V1)

- Multiple chat rooms
- User authentication
- Message reactions
- File/image uploads
- Search functionality
- Admin moderation tools
- Ban system
- Better anti-spam
- Presence indicators (user count)
- Message notifications
