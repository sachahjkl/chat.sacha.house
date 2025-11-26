use anyhow::{Result, bail};
use async_stream::stream;
use futures_util::Stream;
use include_dir::{Dir, include_dir};
use julid::Julid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    collections::HashMap,
    convert::{Infallible, TryInto},
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, broadcast};
use warp::http::StatusCode;
use warp::{Filter, Reply, filters::BoxedFilter, http::Method, sse::Event};

type HttpResponse = warp::reply::Response;

static DIST: Dir = include_dir!("front/dist");
// static SESSION_TIMEOUT_SECONDS: i64 = 10 * 5; // 5 minutes
static SESSION_TIMEOUT_SECONDS: i64 = 60; // 60 seconds (DEBUG)

static MAX_MESSAGE_LENGTH: usize = 240;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    tx: broadcast::Sender<Message>,
    users_tx: broadcast::Sender<UserEvent>,
    rate_limiter: Arc<Mutex<HashMap<String, Instant>>>,
    rate_limit_window: Duration,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Message {
    id: String, // julid.as_string()
    username: String,
    text: String,
    created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct UserEvent {
    action: String, // "ADD" or "REMOVE"
    username: String,
}

#[derive(sqlx::FromRow)]
struct DbMessage {
    id: Julid,
    username: String,
    text: String,
    created_at: i64,
}

#[derive(Deserialize)]
struct ClaimRequest {
    username: String,
}

#[derive(Serialize, Deserialize)]
struct ClaimResponse {
    success: bool,
    session_id: String, // julid.as_string()
    expires_in: i64,
}

#[derive(Serialize)]
struct CurrentUsernameResponse {
    username: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct PostMessageRequest {
    text: String,
}

#[derive(Serialize)]
struct PostMessageResponse {
    success: bool,
    message_id: String, // julid.as_string()
}

#[derive(Deserialize)]
struct GetMessagesQuery {
    from: Option<Julid>,
    limit: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct GetMessagesResponse {
    messages: Vec<Message>,
}

impl AppState {
    async fn check_rate_limit(&self, session_id: &Julid, action: &str) -> bool {
        if self.rate_limit_window.is_zero() {
            return true;
        }
        let now = Instant::now();
        let mut guard = self.rate_limiter.lock().await;
        let key = format!("{}:{}", action, session_id);
        match guard.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if now.duration_since(*entry.get()) < self.rate_limit_window {
                    return false;
                }
                entry.insert(now);
                true
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(now);
                true
            }
            
        }
    }

    async fn clear_rate_limit_key(&self, session_id: &str) {
        let mut guard = self.rate_limiter.lock().await;
        guard.remove(&format!("message:{}", session_id));
        guard.remove(&format!("claim:{}", session_id));
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    pretty_env_logger::init();

    let db_url = "sqlite:db/chat.db?mode=rwc";
    let db = init_db(db_url).await?;

    let (tx, _rx) = broadcast::channel(100);
    let (users_tx, _users_rx) = broadcast::channel(100);

    let rate_limit_secs = std::env::var("RATE_LIMIT_SECS")
        .unwrap_or_else(|_| "1".to_string())
        .parse::<u64>()
        .unwrap_or(1);

    let state = AppState {
        db: db.clone(),
        tx,
        users_tx: users_tx.clone(),
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        rate_limit_window: Duration::from_secs(rate_limit_secs),
    };
    let routes = build_routes(state.clone());

    tokio::spawn({
        let state = state.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let now = unix_now();
                if let Ok(expired) = sqlx::query_as::<_, (String,)>(
                    "SELECT username FROM sessions WHERE last_locked + ? <= ? AND last_locked > 0",
                )
                .bind(SESSION_TIMEOUT_SECONDS)
                .bind(now)
                .fetch_all(&state.db)
                .await
                {
                    for (username,) in expired {
                        let _ = sqlx::query("UPDATE sessions SET last_locked = 0 WHERE username = ?")
                            .bind(&username)
                            .execute(&state.db)
                            .await;
                        let _ = state.users_tx.send(UserEvent {
                            action: "REMOVE".to_string(),
                            username: username.clone(),
                        });
                    }
                }
            }
        }
    });

    let cors = warp::cors()
        .allow_any_origin()
        // list of headers to allow in requests
        .allow_headers(vec![
            "Content-Type",
            "Cookie",
            "Authorization",
            "Origin",
            "Referer",
            "User-Agent",
            "Accept",
            "Accept-Encoding",
            "Accept-Language",
            "Connection",
            "Host",
            "Sec-Fetch-Dest",
            "Access-Control-Request-Headers",
            "Access-Control-Request-Method",
            "Access-Control-Allow-Credentials",
        ])
        .allow_methods(&[Method::GET, Method::POST, Method::OPTIONS]);

    let bind_host = std::env::var("BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let bind_port: u16 = std::env::var("BIND_PORT")
        .unwrap_or_else(|_| "3030".to_string())
        .parse()
        .unwrap_or(3030);

    let bind_addr: [u8; 4] = bind_host
        .split('.')
        .map(|s| s.parse().unwrap_or(127))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap_or([127, 0, 0, 1]);

    log::info!("Server starting on http://{}:{}", bind_host, bind_port);
    warp::serve(routes.with(cors))
        .run((bind_addr, bind_port))
        .await;

    Ok(())
}

fn build_routes(state: AppState) -> BoxedFilter<(HttpResponse,)> {
    let state_filter = warp::any().map(move || state.clone());

    let claim_route = warp::post()
        .and(warp::path!("api" / "username" / "claim"))
        .and(warp::body::json())
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_claim);

    let get_current_username_route = warp::get()
        .and(warp::path!("api" / "username" / "current"))
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_get_current_username);

    let release_route = warp::post()
        .and(warp::path!("api" / "username" / "release"))
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_release);

    let post_message_route = warp::post()
        .and(warp::path!("api" / "messages"))
        .and(warp::body::json())
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_post_message);

    let get_messages_route = warp::get()
        .and(warp::path!("api" / "messages"))
        .and(warp::query())
        .and(state_filter.clone())
        .and_then(handle_get_messages);

    let sse_route = warp::get()
        .and(warp::path!("api" /"messages" /"sse"))
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_sse);

    let get_users_route = warp::get()
        .and(warp::path!("api" / "users"))
        .and(state_filter.clone())
        .and_then(handle_get_users);

    let users_sse_route = warp::get()
        .and(warp::path!("api" / "users" / "sse"))
        .and(state_filter.clone())
        .and_then(handle_users_sse);

    let stats_route = warp::get()
        .and(warp::path!("api" / "stats"))
        .and(state_filter.clone())
        .and_then(handle_stats);

    let static_files = warp::get().and(warp::path::tail()).and_then(handle_static);

    claim_route
        .or(get_current_username_route)
        .unify()
        .or(release_route)
        .unify()
        .or(post_message_route)
        .unify()
        .or(get_messages_route)
        .unify()
        .or(sse_route)
        .unify()
        .or(get_users_route)
        .unify()
        .or(users_sse_route)
        .unify()
        .or(stats_route)
        .unify()
        .or(static_files)
        .unify()
        .boxed()
}

async fn handle_claim(
    req: ClaimRequest,
    cookie_session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    let username = req.username.trim();
    if username.is_empty() || username.len() > 32 {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            &ErrorResponse {
                error: "Invalid username".into(),
            },
        ));
    }

    let now = unix_now();

    let existing = sqlx::query_as::<_, (Julid, i64)>(
        "SELECT session_id, last_locked FROM sessions WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| warp::reject::not_found())?;

    let (final_session_id, is_new_claim) = match existing {
        Some((existing_session_id, last_locked)) => {
            let is_locked = last_locked + SESSION_TIMEOUT_SECONDS > now;

            if is_locked {
                if let Some(cookie_sid) = cookie_session_id {
                    if cookie_sid == existing_session_id {
                        (existing_session_id, false)
                    } else {
                        return Ok(json_response(
                            StatusCode::CONFLICT,
                            &ErrorResponse {
                                error: "Username is taken".into(),
                            },
                        ));
                    }
                } else {
                    return Ok(json_response(
                        StatusCode::CONFLICT,
                        &ErrorResponse {
                            error: "Username is taken".into(),
                        },
                    ));
                }
            } else {
                (Julid::new(), true)
            }
        }
        None => {
            if let Some(cookie_sid) = cookie_session_id {
                if let Ok(Some((existing_username,))) = sqlx::query_as::<_, (String,)>(
                    "SELECT username FROM sessions WHERE session_id = ?",
                )
                .bind(cookie_sid)
                .fetch_optional(&state.db)
                .await
                {
                    if existing_username == username {
                        (cookie_sid, false)
                    } else {
                        (Julid::new(), true)
                    }
                } else {
                    (Julid::new(), true)
                }
            } else {
                (Julid::new(), true)
            }
        }
    };

    sqlx::query("INSERT OR REPLACE INTO sessions (session_id, username, last_locked) VALUES (?, ?, ?)")
        .bind(&final_session_id)
        .bind(username)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?;

    if is_new_claim {
        let _ = state.users_tx.send(UserEvent {
            action: "ADD".to_string(),
            username: username.to_string(),
        });
    }

    let cookie = format!(
        "session_id={}; HttpOnly; Secure; SameSite=Strict; Path=/",
        final_session_id
    );

    let response = ClaimResponse {
        success: true,
        session_id: final_session_id.as_string(),
        expires_in: SESSION_TIMEOUT_SECONDS,
    };

    Ok(
        warp::reply::with_header(warp::reply::json(&response), "Set-Cookie", cookie)
            .into_response(),
    )
}

async fn handle_get_current_username(
    session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    if let Some(sid) = session_id {
        if let Some(username) = get_username(&state, sid).await {
            let now = unix_now();
            let (last_locked,): (i64,) = sqlx::query_as::<_, (i64,)>(
                "SELECT last_locked FROM sessions WHERE session_id = ?",
            )
            .bind(sid)
            .fetch_one(&state.db)
            .await
            .map_err(|_| warp::reject::not_found())?;

            let remaining = (last_locked + SESSION_TIMEOUT_SECONDS) - now;
            let expires_in = if remaining > 0 { Some(remaining) } else { None };

            return Ok(warp::reply::json(&CurrentUsernameResponse {
                username: Some(username),
                expires_in,
            })
            .into_response());
        }
    }

    Ok(warp::reply::json(&CurrentUsernameResponse {
        username: None,
        expires_in: None,
    })
    .into_response())
}

async fn handle_release(
    session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    if let Some(sid) = session_id {
        let rate_key = sid.to_string();
        if let Ok(Some((username,))) = sqlx::query_as::<_, (String,)>(
            "SELECT username FROM sessions WHERE session_id = ?",
        )
        .bind(sid)
        .fetch_optional(&state.db)
        .await
        {
            let _ = sqlx::query("UPDATE sessions SET last_locked = 0 WHERE session_id = ?")
                .bind(sid)
                .execute(&state.db)
                .await;
            let _ = state.users_tx.send(UserEvent {
                action: "REMOVE".to_string(),
                username,
            });
        }
        state.clear_rate_limit_key(&rate_key).await;
    }

    let cookie = "session_id=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0";

    Ok(warp::reply::with_header(
        warp::reply::json(&json!({"success": true})),
        "Set-Cookie",
        cookie,
    )
    .into_response())
}

async fn handle_post_message(
    req: PostMessageRequest,
    session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    let session_id = match session_id {
        Some(s) => s,
        None => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                &ErrorResponse {
                    error: "Unauthorized".into(),
                },
            ));
        }
    };

    let username = match get_username(&state, session_id).await {
        Some(u) => u,
        None => {
            return Ok(json_response(
                StatusCode::UNAUTHORIZED,
                &ErrorResponse {
                    error: "Unauthorized".into(),
                },
            ));
        }
    };

    if !state.check_rate_limit(&session_id, "message").await {
        return Ok(json_response(
            StatusCode::TOO_MANY_REQUESTS,
            &ErrorResponse {
                error: "Too many messages; wait a bit".into(),
            },
        ));
    }

    let text = req.text.trim();
    if text.is_empty() || text.len() > MAX_MESSAGE_LENGTH {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            &ErrorResponse {
                error: "Invalid message length".into(),
            },
        ));
    }

    let msg_id = Julid::new();
    let now = unix_now();

    sqlx::query("INSERT INTO messages (id, username, text, created_at) VALUES (?, ?, ?, ?)")
        .bind(&msg_id)
        .bind(&username)
        .bind(text)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?;

    let _ = sqlx::query("UPDATE sessions SET last_locked = ? WHERE session_id = ?")
        .bind(now)
        .bind(session_id)
        .execute(&state.db)
        .await;

    let msg = Message {
        id: msg_id.as_string(),
        username,
        text: text.to_string(),
        created_at: now,
    };

    let _ = state.tx.send(msg.clone());

    Ok(warp::reply::json(&PostMessageResponse {
        success: true,
        message_id: msg_id.as_string(),
    })
    .into_response())
}

async fn handle_get_messages(
    query: GetMessagesQuery,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    let limit = query.limit.unwrap_or(50).min(100) as i64;

    let messages = if let Some(from_id) = query.from {
        sqlx::query_as::<_, DbMessage>(
            "SELECT id, username, text, created_at FROM messages WHERE id < ? ORDER BY id DESC LIMIT ?"
        )
        .bind(from_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?
    } else {
        sqlx::query_as::<_, DbMessage>(
            "SELECT id, username, text, created_at FROM messages ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?
    };

    let messages = messages
        .into_iter()
        .map(|m| Message {
            id: m.id.as_string(),
            username: m.username,
            text: m.text,
            created_at: m.created_at,
        })
        .collect();

    Ok(warp::reply::json(&GetMessagesResponse { messages }).into_response())
}

fn sse_events(
    mut rx: broadcast::Receiver<Message>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if let Ok(data) = serde_json::to_string(&msg) {
                        yield Ok(Event::default().event("message").data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Client lagging, they will re-fetch via API.
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

async fn handle_sse(
    session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    let session_id = match session_id {
        Some(s) => s,
        None => {
            return Ok(unauthorized());
        }
    };
    if get_username(&state, session_id).await.is_none() {
        return Ok(unauthorized());
    }

    let rx = state.tx.subscribe();
    let stream = sse_events(rx);

    Ok(warp::sse::reply(warp::sse::keep_alive().stream(stream)).into_response())
}

async fn handle_get_users(state: AppState) -> Result<HttpResponse, warp::Rejection> {
    match get_active_usernames(&state).await {
        Ok(usernames) => Ok(warp::reply::json(&json!({ "users": usernames })).into_response()),
        Err(_) => Ok(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &ErrorResponse {
                error: "Failed to fetch users".into(),
            },
        )),
    }
}

fn users_sse_events(
    mut rx: broadcast::Receiver<UserEvent>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().event("user").data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Client lagging, they will re-fetch via API.
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

async fn handle_users_sse(state: AppState) -> Result<HttpResponse, warp::Rejection> {
    let rx = state.users_tx.subscribe();
    let stream = users_sse_events(rx);

    Ok(warp::sse::reply(warp::sse::keep_alive().stream(stream)).into_response())
}

async fn handle_stats(state: AppState) -> Result<HttpResponse, warp::Rejection> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?;

    Ok(warp::reply::json(&json!({ "total_messages": count.0 })).into_response())
}

async fn handle_static(path: warp::path::Tail) -> Result<HttpResponse, warp::Rejection> {
    let path_str = path.as_str();
    let file_path = if path_str.is_empty() {
        "index.html"
    } else {
        path_str
    };

    if let Some(file) = DIST.get_file(file_path) {
        let mime = mime_guess::from_path(file_path).first_or_octet_stream();
        return Ok(
            warp::reply::with_header(file.contents(), "Content-Type", mime.as_ref())
                .into_response(),
        );
    }

    if let Some(file) = DIST.get_file("index.html") {
        return Ok(
            warp::reply::with_header(file.contents(), "Content-Type", "text/html").into_response(),
        );
    }

    Err(warp::reject::not_found())
}

async fn get_username(state: &AppState, session_id: Julid) -> Option<String> {
    let now = unix_now();
    sqlx::query_as::<_, (String, i64)>(
        "SELECT username, last_locked FROM sessions WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .and_then(|(username, last_locked)| {
        if last_locked + SESSION_TIMEOUT_SECONDS > now {
            Some(username)
        } else {
            None
        }
    })
}

async fn get_active_usernames(state: &AppState) -> Result<Vec<String>> {
    let now = unix_now();
    let usernames: Vec<(String,)> = sqlx::query_as(
        "SELECT username FROM sessions WHERE last_locked + ? > ? AND last_locked > 0",
    )
    .bind(SESSION_TIMEOUT_SECONDS)
    .bind(now)
    .fetch_all(&state.db)
    .await?;
    Ok(usernames.into_iter().map(|(u,)| u).collect())
}

async fn init_db(db_url: &str) -> Result<SqlitePool> {
    ensure_sqlite_file(db_url)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
async fn init_in_memory_db() -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA busy_timeout = 5000;")
        .execute(pool)
        .await?;

    ensure_migrations_table(pool).await?;

    if !migration_applied(pool, 1).await? {
        migrate_create_tables(pool).await?;
        record_migration(pool, 1, "migrate_create_tables").await?;
    }

    if !migration_applied(pool, 2).await? {
        migrate_sessions_last_locked(pool).await?;
        record_migration(pool, 2, "migrate_sessions_last_locked").await?;
    }

    if !session_column_exists(pool, "last_locked").await?
        || session_column_exists(pool, "locked").await?
        || session_column_exists(pool, "last_activity").await?
    {
        bail!("sessions table failed to reach last_locked schema");
    }

    Ok(())
}

async fn ensure_migrations_table(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn migration_applied(pool: &SqlitePool, version: i64) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM migrations WHERE version = ?;")
        .bind(version)
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

async fn record_migration(pool: &SqlitePool, version: i64, name: &str) -> Result<()> {
    sqlx::query("INSERT INTO migrations (version, name) VALUES (?, ?);")
        .bind(version)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

async fn migrate_create_tables(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            session_id BLOB NOT NULL PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            locked INTEGER NOT NULL DEFAULT 1,
            last_activity INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) STRICT;",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_username ON sessions(username);")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id BLOB NOT NULL PRIMARY KEY,
            username TEXT NOT NULL,
            text TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(username) REFERENCES sessions(username)
        ) STRICT;",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);")
        .execute(pool)
        .await?;

    Ok(())
}

async fn migrate_sessions_last_locked(pool: &SqlitePool) -> Result<()> {
    let has_last_locked = session_column_exists(pool, "last_locked").await?;
    let has_locked = session_column_exists(pool, "locked").await?;
    let has_last_activity = session_column_exists(pool, "last_activity").await?;

    if has_last_locked && !has_locked && !has_last_activity {
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_username ON sessions(username);")
            .execute(pool)
            .await?;
        return Ok(());
    }

    let mut pooled = pool.acquire().await?;
    let raw_conn = pooled.as_mut();
    sqlx::query("PRAGMA foreign_keys = OFF;")
        .execute(&mut *raw_conn)
        .await?;
    sqlx::query("BEGIN IMMEDIATE;")
        .execute(&mut *raw_conn)
        .await?;

    let migrate = async {
        sqlx::query(
            "CREATE TABLE sessions_new (
                session_id BLOB NOT NULL PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                last_locked INTEGER NOT NULL DEFAULT 0
            ) STRICT;",
        )
        .execute(&mut *raw_conn)
        .await?;

        if has_last_locked {
            sqlx::query(
                "INSERT INTO sessions_new (session_id, username, last_locked)
                 SELECT session_id, username, last_locked FROM sessions;",
            )
            .execute(&mut *raw_conn)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO sessions_new (session_id, username, last_locked)
                 SELECT session_id,
                        username,
                        CASE WHEN locked = 1 THEN last_activity ELSE 0 END
                 FROM sessions;",
            )
            .execute(&mut *raw_conn)
            .await?;
        }

        sqlx::query("DROP TABLE sessions;")
            .execute(&mut *raw_conn)
            .await?;

        sqlx::query("ALTER TABLE sessions_new RENAME TO sessions;")
            .execute(&mut *raw_conn)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_username ON sessions(username);")
            .execute(&mut *raw_conn)
            .await?;

        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&mut *raw_conn)
            .await?;

        Ok::<(), sqlx::Error>(())
    }
    .await;

    match migrate {
        Ok(_) => {
            sqlx::query("COMMIT;").execute(&mut *raw_conn).await?;
            Ok(())
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK;").execute(&mut *raw_conn).await;
            let _ = sqlx::query("PRAGMA foreign_keys = ON;")
                .execute(&mut *raw_conn)
                .await;
            Err(err.into())
        }
    }
}

async fn session_column_exists(pool: &SqlitePool, column: &str) -> Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = ?;")
            .bind(column)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

fn unauthorized() -> HttpResponse {
    json_response(
        StatusCode::UNAUTHORIZED,
        &ErrorResponse {
            error: "Unauthorized".into(),
        },
    )
}

fn ensure_sqlite_file(db_url: &str) -> Result<()> {
    if let Some(rest) = db_url.strip_prefix("sqlite:") {
        if rest.starts_with(':') {
            return Ok(());
        }
        let path_part = rest.split('?').next().unwrap_or(rest);
        if path_part.is_empty() {
            return Ok(());
        }
        let path = Path::new(path_part);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        if !path.exists() {
            fs::File::create(path)?;
        }
    }
    Ok(())
}

fn json_response<T: Serialize>(status: StatusCode, payload: &T) -> HttpResponse {
    warp::reply::with_status(warp::reply::json(payload), status).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::from_slice;
    use warp::test::request;

    #[tokio::test]
    async fn claim_username_success() {
        let (app, _state) = setup_test_app().await;

        let resp = request()
            .method("POST")
            .path("/api/username/claim")
            .header("content-type", "application/json")
            .body(r#"{"username":"grug"}"#)
            .reply(&app)
            .await;

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("set-cookie").is_some());
    }

    #[tokio::test]
    async fn claim_username_conflict_when_locked() {
        let (app, _state) = setup_test_app().await;

        let first = request()
            .method("POST")
            .path("/api/username/claim")
            .header("content-type", "application/json")
            .body(r#"{"username":"grug"}"#)
            .reply(&app)
            .await;
        assert_eq!(first.status(), StatusCode::OK);

        let second = request()
            .method("POST")
            .path("/api/username/claim")
            .header("content-type", "application/json")
            .body(r#"{"username":"grug"}"#)
            .reply(&app)
            .await;
        assert_eq!(second.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn claim_username_rejects_invalid_payload() {
        let (app, _state) = setup_test_app().await;

        let resp = request()
            .method("POST")
            .path("/api/username/claim")
            .header("content-type", "application/json")
            .body(r#"{"username":""}"#)
            .reply(&app)
            .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn user_can_post_and_fetch_messages() {
        let (app, _state) = setup_test_app().await;

        let claim = request()
            .method("POST")
            .path("/api/username/claim")
            .header("content-type", "application/json")
            .body(r#"{"username":"grug"}"#)
            .reply(&app)
            .await;
        assert_eq!(claim.status(), StatusCode::OK);
        let raw_cookie = claim
            .headers()
            .get("set-cookie")
            .expect("set-cookie")
            .to_str()
            .unwrap();
        let session_cookie = raw_cookie.split(';').next().expect("cookie kv").to_string();

        let texts = vec!["one", "two", "three", "four", "five"];

        for body in texts.iter() {
            let resp = request()
                .method("POST")
                .path("/api/messages")
                .header("content-type", "application/json")
                .header("cookie", &session_cookie)
                .body(format!(r#"{{"text":"{}"}}"#, body))
                .reply(&app)
                .await;
            assert_eq!(resp.status(), StatusCode::OK);
        }

        let resp = request()
            .method("GET")
            .path("/api/messages?limit=10")
            .header("cookie", &session_cookie)
            .reply(&app)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: GetMessagesResponse = from_slice(resp.body()).unwrap();
        assert_eq!(body.messages.len(), texts.len());
        let mut returned: Vec<String> = body.messages.iter().map(|m| m.text.clone()).collect();
        let mut expected: Vec<String> = texts.iter().map(|s| (*s).to_string()).collect();
        returned.sort();
        expected.sort();
        assert_eq!(returned, expected);
        assert!(body.messages.iter().all(|m| m.username == "grug"));
    }

    async fn setup_test_app() -> (BoxedFilter<(HttpResponse,)>, AppState) {
        let db = init_in_memory_db().await.expect("init test db");
        let (tx, _rx) = broadcast::channel(32);
        let (users_tx, _users_rx) = broadcast::channel(32);
        let state = AppState {
            db,
            tx,
            users_tx,
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
            rate_limit_window: Duration::from_millis(0),
        };
        let app = build_routes(state.clone());
        (app, state)
    }
}
