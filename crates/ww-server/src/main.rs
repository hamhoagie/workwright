//! Workwright API server — Rust + SQLite.
//!
//! Same endpoints, same behavior. Single binary, single db file.
//! Reads existing JSONL data on first run (auto-migration).

use std::sync::Arc;
use std::path::PathBuf;

use axum::{
    Router,
    routing::{get, post},
    extract::{Json, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use ww_workspace::{Db, User, Workspace};
use ww_wright::Wright;
use ww_wright::llm::LlmClient;

struct AppState {
    root: PathBuf,
    db: Db,
    workspace: Workspace,
    llm: Option<LlmClient>,
}

type SharedState = Arc<AppState>;

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
        .with_state(state);

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
    headers: HeaderMap,
    Json(body): Json<PostTaskReq>,
) -> impl IntoResponse {
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
    };
    task.taste_score = Some(score);
    task.taste_note = Some(body.reason);
    task.critted_by = Some(user.id.clone());
    task.critted_by_name = Some(user.display_name.clone());
    state.db.update_task(&task).ok();

    // Trust flows to submitter
    if let Some(ref submitter_id) = task.submitted_by {
        state.db.update_trust(submitter_id, score * 0.1).ok();
    }

    // Promote or discard staged files
    let file_scope = task.scope.split(':').next().unwrap_or(&task.scope);
    if score > 0.0 {
        state.workspace.promote(file_scope).ok();
        // TODO: rsync deploy
    } else {
        state.workspace.discard(file_scope);
    }

    Json(serde_json::json!({"ok": true, "score": score})).into_response()
}

async fn get_taste(State(state): State<SharedState>) -> impl IntoResponse {
    match (state.db.all_signals(), state.db.signal_count()) {
        (Ok(signals), Ok(count)) => {
            let guide = build_taste_guide(&signals);
            Json(serde_json::json!({
                "text": guide,
                "signal_count": count,
            }))
            .into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_users(State(state): State<SharedState>) -> impl IntoResponse {
    match state.db.all_users() {
        Ok(users) => {
            let public: Vec<serde_json::Value> = users
                .iter()
                .map(|u| {
                    serde_json::json!({
                        "id": u.id,
                        "display_name": u.display_name,
                        "trust_score": u.trust_score,
                        "role": u.role,
                    })
                })
                .collect();
            Json(public).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_me(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match resolve_user(&headers, &state.db) {
        Some(u) => Json(serde_json::json!({
            "id": u.id,
            "email": u.email,
            "display_name": u.display_name,
            "trust_score": u.trust_score,
            "role": u.role,
        }))
        .into_response(),
        None => (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response(),
    }
}

#[derive(Deserialize)]
struct RegisterReq {
    email: String,
    display_name: String,
}

async fn post_register(
    State(state): State<SharedState>,
    Json(body): Json<RegisterReq>,
) -> impl IntoResponse {
    let user = User {
        id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
        email: body.email,
        display_name: body.display_name,
        token: generate_token(),
        trust_score: 0.0,
        role: "participant".to_string(),
        created: now_secs(),
    };

    match state.db.create_user(&user) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": user.id,
                "display_name": user.display_name,
                "token": user.token,
                "trust_score": user.trust_score,
                "role": user.role,
            })),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_preview(
    State(state): State<SharedState>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    if let Ok(Some(change)) = state.workspace.changelog.get(&change_id) {
        if change.path.ends_with(".html") {
            if let Ok(Some(content)) = state.workspace.staging.read(&change.path) {
                return axum::response::Html(content).into_response();
            }
            if let Some(content) = state.workspace.read_file(&change.path) {
                return axum::response::Html(content).into_response();
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

// --- Helpers ---

fn build_taste_guide(signals: &[ww_workspace::TasteSignal]) -> String {
    if signals.is_empty() {
        return "No taste signals yet. The guide emerges from crit.".to_string();
    }
    let mut guide = format!(
        "## Taste Guide (learned from human feedback)\n\n*Based on {} taste signals.*\n\n",
        signals.len()
    );
    let accepted: Vec<_> = signals.iter().filter(|s| s.score > 0.0).collect();
    let rejected: Vec<_> = signals.iter().filter(|s| s.score <= 0.0).collect();
    if !accepted.is_empty() {
        guide.push_str("**Extracted principles (from accepted work):**\n");
        for s in &accepted {
            guide.push_str(&format!("- {}\n", s.reason));
        }
        guide.push('\n');
    }
    if !rejected.is_empty() {
        guide.push_str("**Anti-patterns (from rejected work):**\n");
        for s in &rejected {
            guide.push_str(&format!("- {}\n", s.reason));
        }
    }
    guide
}

fn build_wright_prompt(
    task: &ww_workspace::Task,
    file_content: &str,
    taste_guide: &str,
) -> String {
    let file_display = if file_content.is_empty() {
        "(new file — create from scratch)"
    } else {
        file_content
    };
    format!(
        r#"You are a wright — a craftsperson who works within a tradition.

## Taste Guide
{taste_guide}

## Your Task
**Intent:** {intent}
**Why:** {why}
**Scope:** {scope}

## Target File Content
```
{file_display}
```

## Instructions
Make the change described in the intent. Follow the principles. Return the complete file content — no explanations, no markdown fences, just the code."#,
        intent = task.intent,
        why = task.why,
        scope = task.scope,
    )
}

fn build_defense_prompt(task: &ww_workspace::Task, code: &str) -> String {
    let truncated = if code.len() > 3000 { &code[..3000] } else { code };
    format!(
        r#"You just completed a piece of work. Now defend it.

**Task:** {intent}
**Why it was needed:** {why}

**What you produced:**
```
{code}
```

Defend your choices. Not what you did — the diff shows that.
Why this form and not another. 2-4 sentences. Conceptual, not technical. Go:"#,
        intent = task.intent,
        why = task.why,
        code = truncated,
    )
}

fn strip_fences(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = if lines.first().is_some_and(|l| l.starts_with("```")) { 1 } else { 0 };
    let end = if lines.last().is_some_and(|l| l.trim() == "```") { lines.len() - 1 } else { lines.len() };
    lines[start..end].join("\n")
}

fn generate_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let a = h.finish();
    let mut h2 = s.build_hasher();
    h2.write_u64(a);
    let b = h2.finish();
    format!("{:x}{:x}", a, b)
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

// --- JSON types ---

#[derive(Serialize)]
struct TaskJson {
    id: String,
    intent: String,
    why: String,
    scope: String,
    status: String,
    created: f64,
    agent_id: Option<String>,
    defense: Option<String>,
    change_ids: Vec<String>,
    taste_score: Option<f64>,
    taste_note: Option<String>,
    submitted_by: Option<String>,
    submitted_by_name: Option<String>,
    critted_by: Option<String>,
    critted_by_name: Option<String>,
}

impl From<ww_workspace::Task> for TaskJson {
    fn from(t: ww_workspace::Task) -> Self {
        Self {
            id: t.id,
            intent: t.intent,
            why: t.why,
            scope: t.scope,
            status: t.status.to_string(),
            created: t.created,
            agent_id: t.agent_id,
            defense: t.defense,
            change_ids: t.change_ids,
            taste_score: t.taste_score,
            taste_note: t.taste_note,
            submitted_by: t.submitted_by,
            submitted_by_name: t.submitted_by_name,
            critted_by: t.critted_by,
            critted_by_name: t.critted_by_name,
        }
    }
}
