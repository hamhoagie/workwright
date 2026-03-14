Looking at this file, I need to add rate limiting middleware for two write endpoints: `POST /api/tasks` (10/hour) and `POST /api/register` (3/hour). The feed stays open.

The right approach: in-process IP-keyed counters with a sliding window. No external dependency. The limiter lives in `AppState` — same pattern as everything else. Two separate limiters because the limits differ. Extract the IP from `X-Forwarded-For` first, fall back to the connection address — but axum's `ConnectInfo` requires that I change `axum::serve` slightly.

Actually, looking at what's available: axum's `ConnectInfo` extractor gives the peer address. I can add that without pulling in new crates. The rate limiter itself: `DashMap` would be clean but it's not a current dependency. A `Mutex<HashMap>` is honest about what it is — process-local state with lock contention — and contention on a rate limit check is not a real cost.

The limiter: bucket per IP, store (count, window_start). On each check, if `now - window_start > 3600`, reset. If `count >= limit`, reject 429. Simple, correct, no theater.

```rust
//! Workwright API server — Rust + SQLite.
//!
//! Same endpoints, same behavior. Single binary, single db file.
//! Reads existing JSONL data on first run (auto-migration).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};

use axum::{
    Router,
    routing::{get, post},
    extract::{ConnectInfo, Json, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use ww_workspace::{Db, User, Workspace};
use ww_wright::Wright;
use ww_wright::llm::LlmClient;

// --- Rate limiting ---

/// A sliding-window rate limiter keyed by IP string.
/// Window resets per IP when an hour has elapsed since first request in window.
struct RateLimiter {
    /// Map from IP → (count_in_window, window_start_secs)
    buckets: Mutex<HashMap<String, (u32, f64)>>,
    limit: u32,
    window_secs: f64,
}

impl RateLimiter {
    fn new(limit: u32, window_secs: f64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            limit,
            window_secs,
        }
    }

    /// Returns true if the request is allowed, false if rate limit exceeded.
    fn check(&self, ip: &str) -> bool {
        let now = now_secs();
        let mut buckets = self.buckets.lock().unwrap();
        let entry = buckets.entry(ip.to_string()).or_insert((0, now));
        if now - entry.1 > self.window_secs {
            // Window expired — reset
            *entry = (1, now);
            return true;
        }
        if entry.0 >= self.limit {
            return false;
        }
        entry.0 += 1;
        true
    }
}

// --- App state ---

struct AppState {
    root: PathBuf,
    db: Db,
    workspace: Workspace,
    llm: Option<LlmClient>,
    /// Rate limiter for POST /api/tasks — 10 per hour per IP.
    brief_limiter: RateLimiter,
    /// Rate limiter for POST /api/register — 3 per hour per IP.
    register_limiter: RateLimiter,
}

type SharedState = Arc<AppState>;

/// Extract the client IP from X-Forwarded-For (first hop) or fall back to
/// the direct peer address. Returns a string suitable for use as a limiter key.
fn client_ip(headers: &HeaderMap, addr: &ConnectInfo<SocketAddr>) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| addr.0.ip().to_string())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let root = std::env::var("WW_ROOT").unwrap_or_else(|_| ".".to_string());
    let root = PathBuf::from(&root)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&root));
    let meta_dir = root.join(".workwright");
    let port: u16 = std::env::var("WW_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8077);

    // Open database
    let db = Db::open(&meta_dir).expect("failed to open database");

    // Auto-migrate from JSONL on first run
    match db.migrate_from_jsonl(&meta_dir) {
        Ok(stats) => {
            if stats.tasks > 0 || stats.signals > 0 || stats.users > 0 {
                tracing::info!(
                    tasks = stats.tasks,
                    signals = stats.signals,
                    users = stats.users,
                    "migrated JSONL → SQLite"
                );
            }
        }
        Err(e) => tracing::warn!("JSONL migration error: {e}"),
    }

    let llm = LlmClient::from_env().ok();
    if llm.is_none() {
        tracing::warn!("ANTHROPIC_API_KEY not set — wrights will not run");
    }

    let site_dir = root.join("site");

    let state = Arc::new(AppState {
        db,
        workspace: Workspace::new(&root),
        llm,
        root: root.clone(),
        brief_limiter: RateLimiter::new(10, 3600.0),
        register_limiter: RateLimiter::new(3, 3600.0),
    });

    let app = Router::new()
        .route("/api/tasks", get(get_tasks))
        .route("/api/tasks", post(post_task))
        .route("/api/crit", post(post_crit))
        .route("/api/taste", get(get_taste))
        .route("/api/users", get(get_users))
        .route("/api/me", get(get_me))
        .route("/api/register", post(post_register))
        .route("/api/preview/{change_id}", get(get_preview))
        .fallback_service(ServeDir::new(&site_dir))
        .layer(CorsLayer::permissive())
        .with_state(state)
        // ConnectInfo must wrap the whole service so extractors can see it
        .into_make_service_with_connect_info::<SocketAddr>();

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Workwright API on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// --- Auth ---

fn resolve_user(headers: &HeaderMap, db: &Db) -> Option<User> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim())?;
    db.get_user_by_token(token).ok().flatten()
}

// --- Handlers ---

async fn get_tasks(State(state): State<SharedState>) -> impl IntoResponse {
    match state.db.all_tasks() {
        Ok(tasks) => Json(tasks.into_iter().map(TaskJson::from).collect::<Vec<_>>()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct PostTaskReq {
    intent: String,
    why: String,
    scope: Option<String>,
}

async fn post_task(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PostTaskReq>,
) -> impl IntoResponse {
    let ip = client_ip(&headers, &ConnectInfo(addr));
    if !state.brief_limiter.check(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "rate limit exceeded — 10 briefs per hour"})),
        )
            .into_response();
    }

    let user = match resolve_user(&headers, &state.db) {
        Some(u) => u,
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response(),
    };

    let scope = body.scope.as_deref().unwrap_or("site/index.html");
    let file_path = scope.split(':').next().unwrap_or(scope);

    let task = ww_workspace::Task {
        id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
        intent: body.intent,
        why: body.why,
        scope: scope.to_string(),
        status: ww_workspace::TaskStatus::Pending,
        created: now_secs(),
        agent_id: None,
        defense: None,
        context: vec![file_path.to_string()],
        change_ids: vec![],
        taste_score: None,
        taste_note: None,
        submitted_by: Some(user.id.clone()),
        submitted_by_name: Some(user.display_name.clone()),
        critted_by: None,
        critted_by_name: None,
    };

    if let Err(e) = state.db.create_task(&task) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    // Spawn wright on dedicated thread with its OWN db connection.
    // WAL mode allows concurrent reads (server) while wright writes.
    if let Some(ref llm) = state.llm {
        let tid = task.id.clone();
        let root = state.root.clone();
        let meta_dir = root.join(".workwright");
        let llm = llm.clone();
        std::thread::spawn(move || {
            let wright_db = Db::open(&meta_dir).expect("wright db connection");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("wright runtime");
            rt.block_on(async {
                run_wright(&wright_db, &root, &llm, &tid).await;
            });
        });
    }

    (StatusCode::CREATED, Json(TaskJson::from(task))).into_response()
}

async fn run_wright(db: &Db, root: &PathBuf, llm: &LlmClient, task_id: &str) {
    // Claim
    let mut task = match db.get_task(task_id) {
        Ok(Some(t)) => t,
        _ => return,
    };
    task.status = ww_workspace::TaskStatus::Active;
    task.agent_id = Some("wright-1".to_string());
    db.update_task(&task).ok();

    let ws = Workspace::new(root);
    let file_scope = task.scope.split(':').next().unwrap_or(&task.scope).to_string();
    let file_content = ws.read_file(&file_scope).unwrap_or_default();

    // Read taste guide from DB signals
    let guide = match db.all_signals() {
        Ok(signals) => build_taste_guide(&signals),
        Err(_) => String::new(),
    };

    // Lock
    if ws.locks.acquire(&file_scope, "wright-1", &task.intent).is_err() {
        task.status = ww_workspace::TaskStatus::Failed;
        task.defense = Some("Could not acquire lock".to_string());
        db.update_task(&task).ok();
        return;
    }

    // LLM calls
    let prompt = build_wright_prompt(&task, &file_content, &guide);
    let new_content = match llm.call(&prompt).await {
        Ok(text) => strip_fences(text.trim()),
        Err(e) => {
            ws.locks.release(&file_scope, "wright-1").ok();
            task.status = ww_workspace::TaskStatus::Failed;
            task.defense = Some(format!("LLM error: {e}"));
            db.update_task(&task).ok();
            return;
        }
    };

    let defense_prompt = build_defense_prompt(&task, &new_content);
    let defense = match llm.call(&defense_prompt).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            ws.locks.release(&file_scope, "wright-1").ok();
            task.status = ww_workspace::TaskStatus::Failed;
            task.defense = Some(format!("Defense LLM error: {e}"));
            db.update_task(&task).ok();
            return;
        }
    };

    // Stage
    ws.write_staged(&file_scope, &new_content, "wright-1", &task.intent).ok();
    ws.locks.release(&file_scope, "wright-1").ok();

    // Submit for review
    task.status = ww_workspace::TaskStatus::Review;
    task.defense = Some(defense);
    db.update_task(&task).ok();

    tracing::info!(task_id, "wright completed");
}

#[derive(Deserialize)]
struct PostCritReq {
    task_id: String,
    score: f64,
    reason: String,
}

async fn post_crit(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<PostCritReq>,
) -> impl IntoResponse {
    let user = match resolve_user(&headers, &state.db) {
        Some(u) if u.is_admin() => u,
        Some(_) => return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "only admins can crit"}))).into_response(),
        None => return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response(),
    };

    let score = body.score.clamp(-1.0, 1.0);

    // Record taste signal
    let signal = ww_workspace::TasteSignal {
        score,
        reason: body.reason.clone(),
        task_id: body.task_id.clone(),
        change_id: None,
        timestamp: now_secs(),
        tags: vec![],
    };
    state.db.record_taste(&signal).ok();

    // Update task
    let mut task = match state.db.get_task(&body.task_id) {
        Ok(Some(t)) => t,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "task not found"}))).into_response(),
    };

    task.status = if score > 0.0 {
        ww_workspace::TaskStatus::Accepted
    } else {
        ww_workspace::TaskStatus::Rejected