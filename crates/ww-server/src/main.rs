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
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

use ww_workspace::{TaskStore, TasteStore, Workspace};

struct AppState {
    workspace: Workspace,
    tasks: TaskStore,
    taste: TasteStore,
    submit_token: String,
}

type SharedState = Arc<RwLock<AppState>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let root = std::env::var("WW_ROOT").unwrap_or_else(|_| ".".to_string());
    let root = PathBuf::from(root);
    let meta_dir = root.join(".workwright");
    let token = std::env::var("WW_SUBMIT_TOKEN").unwrap_or_default();
    let port: u16 = std::env::var("WW_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8077);

    let state = Arc::new(RwLock::new(AppState {
        workspace: Workspace::new(&root),
        tasks: TaskStore::new(&meta_dir),
        taste: TasteStore::new(&meta_dir),
        submit_token: token,
    }));

    let app = Router::new()
        .route("/api/tasks", get(get_tasks))
        .route("/api/tasks", post(post_task))
        .route("/api/crit", post(post_crit))
        .route("/api/taste", get(get_taste))
        .route("/api/preview/{change_id}", get(get_preview))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Workwright API on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
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
    Json(body): Json<PostTaskReq>,
) -> impl IntoResponse {
    let st = state.read().await;

    let scope = body.scope.as_deref().unwrap_or("site/index.html");
    let file_path = scope.split(':').next().unwrap_or(scope);
    let context = vec![file_path.to_string()];

    match st.tasks.create(&body.intent, &body.why, scope, context) {
        Ok(task) => (StatusCode::CREATED, Json(TaskJson::from(task))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
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
    Json(body): Json<PostCritReq>,
) -> impl IntoResponse {
    let st = state.read().await;
    let score = body.score.clamp(-1.0, 1.0);

    // Record taste signal
    if let Err(e) = st.taste.record(score, &body.reason, &body.task_id, None) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Update task status
    match st.tasks.crit(&body.task_id, score, &body.reason) {
        Ok(_task) => {
            // Promote or discard staged files
            let file_scope = _task.scope.split(':').next().unwrap_or(&_task.scope);
            if score > 0.0 {
                st.workspace.promote(file_scope).ok();
            } else {
                st.workspace.discard(file_scope);
            }
            Json(serde_json::json!({"ok": true, "score": score})).into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
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
            // Try staged first
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
