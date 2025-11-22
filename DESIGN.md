# Chat Room Design Document

## Overview

Single global chat room web application with no authentication. Users pick a username which is reserved to their session via HTTP cookies. Real-time message delivery via Server-Sent Events (SSE/EventSource). Self-contained single binary with embedded frontend assets.

## Core Requirements

- **No Authentication**: Usernames only, no passwords or accounts
- **Session-Based Username Reservation**: Username locked to session cookie, not IP address
- **Real-Time Updates**: SSE for live message streaming
- **Infinite Message History**: All messages persisted, pagination for scrollback
- **Self-Contained**: Single executable with embedded Svelte frontend
- **Simple Architecture**: Grug brain philosophy - avoid complexity demon

## Architecture

### Backend (Rust + warp)

- **Web Framework**: warp for HTTP server and routing
- **Database**: SQLite with best practices (WAL mode, foreign keys, busy timeout)
- **Real-Time**: tokio::sync::broadcast queue for pub/sub message distribution
- **Message Ordering**: UUIDv7 for timestamp-based sortable IDs
- **Rate Limiting**: (Removed as per simplified requirements)
- **Session Management**: HTTP-only, Secure cookies with UUIDv4 session IDs

### Frontend (Svelte)

- **Framework**: Svelte (user will init in `front/` subdirectory)
- **Build**: Bun + Svelte, dist embedded via include_dir
- **Real-Time**: EventSource API for SSE connection
- **Virtual Scrolling**: Svelte library for efficient history rendering
- **XSS Protection**: textContent only, never innerHTML

## Database Schema

### sessions table

```sql
CREATE TABLE sessions (
  session_id BLOB PRIMARY KEY,        -- JULID
  username TEXT NOT NULL UNIQUE,      -- Unique username
  locked INTEGER NOT NULL DEFAULT 1,  -- 1=active, 0=released
  last_activity_seconds INTEGER NOT NULL DEFAULT 0     -- Unix timestamp (seconds)
) STRICT;

CREATE INDEX idx_sessions_username ON sessions(username);
```

### messages table

```sql
CREATE TABLE messages (
  id BLOB PRIMARY KEY,                -- JULID (superset of ulid, compatible, convertible to uuidv7) (sortable by creation time)
  username TEXT NOT NULL,             -- Foreign key to sessions
  text TEXT NOT NULL,                 -- Message content
  created_at INTEGER NOT NULL,        -- Unix timestamp (seconds)
  FOREIGN KEY(username) REFERENCES sessions(username)
) STRICT;

CREATE INDEX idx_messages_created ON messages(created_at);
```

## API Endpoints

### POST /api/username/claim

Claim a username and establish session.

**Request Body**: `{"username": "string"}`

**Response**: `{"success": true, "session_id": "uuid"}`

**Side Effects**:

- Validates username availability
- Generates session_id (UUIDv4)
- Sets cookie: `session_id=<uuid>; HttpOnly; Secure; SameSite=Strict`
- Inserts session into DB with locked=1

### POST /api/username/release

Manually release username and end session.

**Response**: `{"success": true}`

**Side Effects**:

- Sets locked=0 for current session
- Clears session cookie

### POST /api/messages

Send a message to the chat room.

**Request Body**: `{"text": "string"}`

**Response**: `{"success": true, "message_id": "uuid"}`

**Flow**:

1. Validate session cookie exists
2. Validate message: `length(trimmed(text)) <= 240` and non-empty
3. Generate UUIDv7 for message ID
4. Insert message into DB
5. Broadcast message to all SSE clients via queue

### GET /api/messages

Retrieve message history with pagination.

**Query Parameters**:

- `from`: UUIDv7 message ID (optional, for pagination)
- `limit`: number (default 50, max 100)

**Response**: `{"messages": [{id, username, text, created_at}, ...]}`

Returns messages ordered by UUIDv7 ID descending (newest first).

### GET /api/sse

Server-Sent Events stream for real-time messages.

**Headers**: Cookie with session_id required

**Event Format**:

```
event: message
data: {"id": "uuid", "username": "string", "text": "string", "created_at": 1234567890}

event: ping
data: {"timestamp": 1234567890}
```

**Keep-Alive**: Periodic ping events every 30s

**Disconnection Handling**: After 10s timeout, sets locked=0 for session

### GET /api/stats

Get global statistics.

**Response**: `{"total_messages": 12345}`

### GET /

Serve embedded Svelte application (index.html and assets).

## User Flow

### Initial Connection

1. User visits `/`
2. Frontend checks for session cookie
3. If no session, prompt for username
4. User enters username
5. POST to `/api/username/claim`
6. Server validates availability, generates session_id, sets cookie
7. Frontend establishes EventSource to `/api/sse`
8. Frontend fetches initial messages from `/api/messages`

### Sending Messages

1. User types message (max 240 chars)
2. Client-side validation (trimmed length, non-empty)
3. POST to `/api/messages`
4. Server validates, inserts to DB
5. Server broadcasts to all SSE connections
6. All clients receive and display message via SSE

### Receiving Messages

1. EventSource connection receives `message` events
2. Parse JSON data
3. Append to chat UI using `textContent` (XSS-safe)
4. Handle `RecvError::Lagged` by re-fetching from `/api/messages`

### Scrolling History

1. User scrolls to top of message list
2. Virtual scroller detects need for more data
3. GET `/api/messages?from=<oldest_visible_id>&limit=50`
4. Prepend messages to virtual scroll list

### Manual Logout

1. User clicks logout button
2. POST to `/api/username/release`
3. Server sets locked=0, clears cookie
4. Frontend closes EventSource, redirects to username prompt

### Automatic Session Timeout

1. SSE connection drops (network, browser close, etc.)
2. Server detects disconnection
3. After 10s grace period (allows reconnection), sets locked=0
4. Username becomes available for others

## Message Validation

### Server-Side (Enforced)

- Message text after trim must be <= 240 characters
- Message text after trim must be non-empty
- Session cookie must be valid
- Username must be locked (active session)

### Client-Side (UX)

- Same validation as server
- Real-time character counter
- Disable send button if invalid

## Security Considerations

### Session Management

- **HttpOnly**: Prevents JavaScript access to session cookie
- **Secure**: Cookie only sent over HTTPS in production
- **SameSite=Strict**: CSRF protection
- **Session IDs**: UUIDv4 (cryptographically random)

### XSS Protection

- Frontend MUST use `textContent` to render messages
- NEVER use `innerHTML` or `v-html` or similar
- Message content treated as plain text only

### Rate Limiting

(Removed)

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
warp = "0.3.7"
tokio = { version = "1.48.0", features = ["full"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.145"
include_dir = "0.7.4"
futures-util = "0.3.31"
warp-rate-limit = "0.3.0"
anyhow = "1.0.100"
thiserror = "2.0.17"
sqlx = { version = "0.8.2", features = ["runtime-tokio", "sqlite", "macros"] }
julid-rs = "1.6.1803398874989"
log = "0.4.28"
env_logger = "0.11.8"
async-stream = "0.3.6"
mime_guess = "2.0.5"
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
