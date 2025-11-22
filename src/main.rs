use anyhow::Result;
use async_stream::stream;
use futures_util::Stream;
use include_dir::{Dir, include_dir};
use julid::Julid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    collections::HashMap,
    convert::Infallible,
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

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    tx: broadcast::Sender<Message>,
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
    async fn check_rate_limit(&self, session_id: &Julid) -> bool {
        if self.rate_limit_window.is_zero() {
            return true;
        }
        let now = Instant::now();
        let mut guard = self.rate_limiter.lock().await;
        let entry = guard.entry(session_id.to_string()).or_insert_with(|| now);
        if now.duration_since(*entry) < self.rate_limit_window {
            return false;
        }
        *entry = now;
        true
    }

    async fn clear_rate_limit_key(&self, key: &str) {
        let mut guard = self.rate_limiter.lock().await;
        guard.remove(key);
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

    let db_url = "sqlite:chat.db?mode=rwc";
    let db = init_db(db_url).await?;

    let (tx, _rx) = broadcast::channel(100);

    let state = AppState {
        db,
        tx,
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        rate_limit_window: Duration::from_secs(1),
    };
    let routes = build_routes(state);

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

    log::info!("Server starting on http://127.0.0.1:3030");
    warp::serve(routes.with(cors))
        .run(([127, 0, 0, 1], 3030))
        .await;

    Ok(())
}

fn build_routes(state: AppState) -> BoxedFilter<(HttpResponse,)> {
    let state_filter = warp::any().map(move || state.clone());

    let claim_route = warp::post()
        .and(warp::path!("api" / "username" / "claim"))
        .and(warp::body::json())
        .and(state_filter.clone())
        .and_then(handle_claim);

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
        .and(warp::path!("api" / "sse"))
        .and(warp::cookie::optional("session_id"))
        .and(state_filter.clone())
        .and_then(handle_sse);

    let stats_route = warp::get()
        .and(warp::path!("api" / "stats"))
        .and(state_filter.clone())
        .and_then(handle_stats);

    let static_files = warp::get().and(warp::path::tail()).and_then(handle_static);

    claim_route
        .or(release_route)
        .unify()
        .or(post_message_route)
        .unify()
        .or(get_messages_route)
        .unify()
        .or(sse_route)
        .unify()
        .or(stats_route)
        .unify()
        .or(static_files)
        .unify()
        .boxed()
}

async fn handle_claim(req: ClaimRequest, state: AppState) -> Result<HttpResponse, warp::Rejection> {
    let username = req.username.trim();
    if username.is_empty() || username.len() > 32 {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            &ErrorResponse {
                error: "Invalid username".into(),
            },
        ));
    }

    let session_id = Julid::new();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let existing = sqlx::query_as::<_, (i64, i64)>(
        "SELECT locked, last_activity FROM sessions WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| warp::reject::not_found())?;

    match existing {
        Some((locked, last_activity)) => {
            let is_expired = last_activity + SESSION_TIMEOUT_SECONDS < now;

            if locked == 1 && !is_expired {
                return Ok(json_response(
                    StatusCode::CONFLICT,
                    &ErrorResponse {
                        error: "Username is taken".into(),
                    },
                ));
            }
            sqlx::query("UPDATE sessions SET session_id = ?, locked = 1, last_activity = ? WHERE username = ?")
                .bind(&session_id)
                .bind(now)
                .bind(username)
                .execute(&state.db)
                .await
                .map_err(|_| warp::reject::not_found())?;
        }
        None => {
            sqlx::query("INSERT INTO sessions (session_id, username, locked, last_activity) VALUES (?, ?, 1, ?)")
                .bind(&session_id)
                .bind(username)
                .bind(now)
                .execute(&state.db)
                .await
                .map_err(|_| warp::reject::not_found())?;
        }
    }

    let cookie = format!(
        "session_id={}; HttpOnly; Secure; SameSite=Strict; Path=/",
        session_id
    );

    let response = ClaimResponse {
        success: true,
        session_id: session_id.as_string(),
        expires_in: SESSION_TIMEOUT_SECONDS,
    };

    Ok(
        warp::reply::with_header(warp::reply::json(&response), "Set-Cookie", cookie)
            .into_response(),
    )
}

async fn handle_release(
    session_id: Option<Julid>,
    state: AppState,
) -> Result<HttpResponse, warp::Rejection> {
    if let Some(sid) = session_id {
        let rate_key = sid.to_string();
        let _ = sqlx::query("UPDATE sessions SET locked = 0 WHERE session_id = ?")
            .bind(sid)
            .execute(&state.db)
            .await;
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

    if !state.check_rate_limit(&session_id).await {
        return Ok(json_response(
            StatusCode::TOO_MANY_REQUESTS,
            &ErrorResponse {
                error: "Too many messages; wait a bit".into(),
            },
        ));
    }

    let text = req.text.trim();
    if text.is_empty() || text.len() > 240 {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            &ErrorResponse {
                error: "Invalid message length".into(),
            },
        ));
    }

    let msg_id = Julid::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    sqlx::query("INSERT INTO messages (id, username, text, created_at) VALUES (?, ?, ?, ?)")
        .bind(&msg_id)
        .bind(&username)
        .bind(text)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(|_| warp::reject::not_found())?;

    let _ = sqlx::query("UPDATE sessions SET last_activity = ? WHERE session_id = ?")
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
    sqlx::query_as::<_, (String,)>(
        "SELECT username FROM sessions WHERE session_id = ? AND locked = 1",
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|(u,)| u)
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
        let state = AppState {
            db,
            tx,
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
            rate_limit_window: Duration::from_millis(0),
        };
        let app = build_routes(state.clone());
        (app, state)
    }
}
