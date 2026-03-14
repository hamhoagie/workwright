//! Workwright API server.
//!
//! Same endpoints as the Python version, same `.workwright/` format.
//! Drop-in replacement — reads what Python wrote.

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
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use ww_workspace::{TaskStore, TasteStore, Workspace};
use ww_wright::Wright;
use ww_wright::llm::LlmClient;

struct AppState {
    root: PathBuf,
    workspace: Workspace,
    tasks: TaskStore,
    taste: TasteStore,
    submit_token: String,
    llm: Option<LlmClient>,
}

type SharedState = Arc<RwLock<AppState>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let root = std::env::var("WW_ROOT").unwrap_or_else(|_| ".".to_string());
    let root = PathBuf::from(&root).canonicalize().unwrap_or_else(|_| PathBuf::from(&root));
    let meta_dir = root.join(".workwright");
    let token = std::env::var("WW_SUBMIT_TOKEN").unwrap_or_default();
    let port: u16 = std::env::var("WW_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8077);

    let llm = LlmClient::from_env().ok();
    if llm.is_none() {
        tracing::warn!("ANTHROPIC_API_KEY not set — wrights will not run");
    }

    let site_dir = root.join("site");

    let state = Arc::new(RwLock::new(AppState {
        workspace: Workspace::new(&root),
        tasks: TaskStore::new(&meta_dir),
        taste: TasteStore::new(&meta_dir),
        submit_token: token,
        llm,
        root: root.clone(),
    }));

    let app = Router::new()
        // API routes
        .route("/api/tasks", get(get_tasks))
        .route("/api/tasks", post(post_task))
        .route("/api/crit", post(post_crit))
        .route("/api/taste", get(get_taste))
        .route("/api/preview/{change_id}", get(get_preview))
        // Static site files
        .fallback_service(ServeDir::new(&site_dir))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Workwright API on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// --- Auth ---

fn check_auth(headers: &HeaderMap, token: &str) -> bool {
    if token.is_empty() {
        return true; // no token configured = open
    }
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim() == token)
        .unwrap_or(false)
}

// --- Handlers ---

async fn get_tasks(State(state): State<SharedState>) -> impl IntoResponse {
    let st = state.read().await;
    match st.tasks.all() {
        Ok(tasks) => {
            let out: Vec<TaskJson> = tasks.into_iter().map(TaskJson::from).collect();
            Json(out).into_response()
        }
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
    let (task_id, root, llm) = {
        let st = state.read().await;

        if !check_auth(&headers, &st.submit_token) {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response();
        }

        let scope = body.scope.as_deref().unwrap_or("site/index.html");
        let file_path = scope.split(':').next().unwrap_or(scope);
        let context = vec![file_path.to_string()];

        match st.tasks.create(&body.intent, &body.why, scope, context) {
            Ok(task) => {
                let task_json = TaskJson::from(task.clone());
                let task_id = task.id.clone();
                let root = st.root.clone();
                let llm = st.llm.clone();

                // Return early with response, spawn wright below
                (task_id, root, llm)
            }
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        }
    };

    // Spawn wright on a dedicated thread with its own runtime.
    // This prevents wright file I/O from blocking the server's event loop.
    if let Some(llm) = llm {
        let tid = task_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("wright runtime");
            rt.block_on(async {
                let wright = Wright::new(&root, llm);
                let result = wright.work(&tid).await;
                if result.success {
                    tracing::info!(task_id = %tid, "wright completed");
                } else {
                    tracing::warn!(task_id = %tid, msg = %result.message, "wright failed");
                }
            });
        });
    }

    // Re-read the task to return it
    let st = state.read().await;
    match st.tasks.get(&task_id) {
        Ok(Some(task)) => (StatusCode::CREATED, Json(TaskJson::from(task))).into_response(),
        _ => (StatusCode::CREATED, Json(serde_json::json!({"id": task_id}))).into_response(),
    }
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
    let st = state.read().await;

    if !check_auth(&headers, &st.submit_token) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response();
    }

    let score = body.score.clamp(-1.0, 1.0);

    // Record taste signal
    if let Err(e) = st.taste.record(score, &body.reason, &body.task_id, None) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    // Update task status
    match st.tasks.crit(&body.task_id, score, &body.reason) {
        Ok(task) => {
            let file_scope = task.scope.split(':').next().unwrap_or(&task.scope);
            if score > 0.0 {
                st.workspace.promote(file_scope).ok();
                // TODO: deploy to web server (rsync)
            } else {
                st.workspace.discard(file_scope);
            }
            Json(serde_json::json!({"ok": true, "score": score})).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_taste(State(state): State<SharedState>) -> impl IntoResponse {
    let st = state.read().await;
    match (st.taste.guide(), st.taste.patterns()) {
        (Ok(guide), Ok(patterns)) => Json(serde_json::json!({
            "text": guide,
            "signal_count": patterns.signal_count,
            "likes": patterns.likes,
            "dislikes": patterns.dislikes,
        }))
        .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_preview(
    State(state): State<SharedState>,
    Path(change_id): Path<String>,
) -> impl IntoResponse {
    let st = state.read().await;
    if let Ok(Some(change)) = st.workspace.changelog.get(&change_id) {
        if change.path.ends_with(".html") {
            if let Ok(Some(content)) = st.workspace.staging.read(&change.path) {
                return axum::response::Html(content).into_response();
            }
            if let Some(content) = st.workspace.read_file(&change.path) {
                return axum::response::Html(content).into_response();
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
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
