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
        .route("/api/taste/search", get(get_taste_search))
        .route("/api/users", get(get_users))
        .route("/api/me", get(get_me))
        .route("/api/register", post(post_register))
        .route("/api/preview/{change_id}", get(get_preview))
        .route("/api/diff/{task_id}", get(get_diff))
        .route("/api/render/{task_id}", get(get_render))
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
        feedback: vec![],
        attempts: 0,
    };

    if let Err(e) = state.db.create_task(&task) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    // Embed the task intent (non-blocking, best effort)
    if let Some(ref llm) = state.llm {
        let db = state.db.clone();
        let llm = llm.clone();
        let t = task.clone();
        tokio::spawn(async move { embed_task(&db, &llm, &t).await; });
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

    // Collect all scoped files — support comma-separated multi-file scope
    let raw_scope = task.scope.split(':').next().unwrap_or(&task.scope);
    let file_paths: Vec<String> = raw_scope
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Read all files for context
    let mut files: Vec<(String, String)> = Vec::new();
    for path in &file_paths {
        let content = ws.read_file(path).unwrap_or_default();
        files.push((path.clone(), content));
    }

    // Build taste guide — semantic retrieval if embeddings available, full dump as fallback
    let guide = build_contextual_guide(db, llm, &task).await;

    // LLM call
    let prompt = build_wright_prompt(&task, &files, &guide);
    let llm_output = match llm.call(&prompt).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            task.status = ww_workspace::TaskStatus::Failed;
            task.defense = Some(format!("LLM error: {e}"));
            db.update_task(&task).ok();
            return;
        }
    };

    // Parse multi-file output and apply changes
    let file_changes = parse_multi_file_output(&llm_output, &files);

    if file_changes.is_empty() {
        task.status = ww_workspace::TaskStatus::Failed;
        task.defense = Some("Wright produced no usable changes".to_string());
        db.update_task(&task).ok();
        return;
    }

    // Stage all files
    let mut staged_paths = Vec::new();
    for (path, content) in &file_changes {
        if let Err(e) = ws.write_staged(path, content, "wright-1", &task.intent) {
            tracing::warn!(path, error = %e, "failed to stage file");
        } else {
            staged_paths.push(path.clone());
        }
    }

    if staged_paths.is_empty() {
        task.status = ww_workspace::TaskStatus::Failed;
        task.defense = Some("Failed to stage any files".to_string());
        db.update_task(&task).ok();
        return;
    }

    // Defense
    let defense_prompt = build_defense_prompt(&task, &llm_output);
    let defense = match llm.call(&defense_prompt).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            task.status = ww_workspace::TaskStatus::Failed;
            task.defense = Some(format!("Defense LLM error: {e}"));
            db.update_task(&task).ok();
            return;
        }
    };

    // Submit for review
    task.status = ww_workspace::TaskStatus::Review;
    task.defense = Some(defense);
    // Store which files were changed
    task.change_ids = staged_paths.clone();
    db.update_task(&task).ok();

    tracing::info!(task_id, files = ?staged_paths, "wright completed");
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

    // Embed the taste signal (non-blocking)
    if let Some(ref llm) = state.llm {
        let db = state.db.clone();
        let llm = llm.clone();
        let sig = signal.clone();
        tokio::spawn(async move { embed_taste_signal(&db, &llm, &sig).await; });
    }

    // Update task
    let mut task = match state.db.get_task(&body.task_id) {
        Ok(Some(t)) => t,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "task not found"}))).into_response(),
    };

    task.taste_score = Some(score);
    task.taste_note = Some(body.reason.clone());
    task.critted_by = Some(user.id.clone());
    task.critted_by_name = Some(user.display_name.clone());

    // Collect all file paths involved (change_ids now stores staged paths)
    let raw_scope = task.scope.split(':').next().unwrap_or(&task.scope);
    let mut file_paths: Vec<String> = raw_scope
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // Also include any paths stored in change_ids (from multi-file wrights)
    for cid in &task.change_ids {
        if !file_paths.contains(cid) {
            file_paths.push(cid.clone());
        }
    }

    if score > 0.0 {
        // Accepted — promote ALL staged files, deploy
        task.status = ww_workspace::TaskStatus::Accepted;
        state.db.update_task(&task).ok();

        if let Some(ref submitter_id) = task.submitted_by {
            state.db.update_trust(submitter_id, score * 0.1).ok();
        }

        let mut deployed = false;
        for path in &file_paths {
            if state.workspace.promote(path).unwrap_or(false) {
                deployed = true;
            }
        }
        if deployed {
            deploy_site(&state.root);
        }
    } else {
        // Rejected — discard ALL staged files, add feedback, retry
        for path in &file_paths {
            state.workspace.discard(path);
        }
        task.feedback.push(body.reason.clone());
        task.attempts += 1;

        const MAX_ATTEMPTS: u32 = 3;
        if task.attempts >= MAX_ATTEMPTS {
            task.status = ww_workspace::TaskStatus::Failed;
            task.defense = Some(format!(
                "Failed after {} attempts. Last rejection: {}",
                task.attempts, body.reason
            ));
            state.db.update_task(&task).ok();

            // Distill a meta-lesson from the failure pattern
            if let Some(ref llm) = state.llm {
                let feedback = task.feedback.clone();
                let intent = task.intent.clone();
                let db = state.db.clone();
                let llm = llm.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("lesson runtime");
                    rt.block_on(async {
                        distill_lesson(&db, &llm, &intent, &feedback).await;
                    });
                });
            }
        } else {
            // Reset for retry — wright will pick it up again
            task.status = ww_workspace::TaskStatus::Pending;
            task.agent_id = None;
            task.defense = None;
            state.db.update_task(&task).ok();

            // Re-trigger wright — escalate model on final attempt
            if let Some(ref llm) = state.llm {
                let tid = task.id.clone();
                let root = state.root.clone();
                let meta_dir = root.join(".workwright");
                let attempts = task.attempts;

                // Escalate: after 2 failures, bring in Opus for the last try
                let wright_llm = if attempts >= 2 {
                    tracing::info!(task_id = %tid, "escalating to opus for final attempt");
                    llm.with_model("claude-opus-4-6")
                } else {
                    llm.clone()
                };

                std::thread::spawn(move || {
                    let wright_db = Db::open(&meta_dir).expect("wright db");
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("wright runtime");
                    rt.block_on(async {
                        run_wright(&wright_db, &root, &wright_llm, &tid).await;
                    });
                });
            }
        }
    }

    Json(serde_json::json!({
        "ok": true,
        "score": score,
        "retrying": score < 0.0 && task.attempts < 3,
        "attempt": task.attempts,
    })).into_response()
}

async fn distill_lesson(db: &Db, llm: &LlmClient, intent: &str, feedback: &[String]) {
    let rejections = feedback.iter()
        .enumerate()
        .map(|(i, f)| format!("  {}. {}", i + 1, f))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"A wright attempted this task 3 times and was rejected every time.

**Task:** {intent}

**Rejection reasons:**
{rejections}

Distill this into ONE principle — a short, sharp lesson that prevents this failure pattern from recurring. Not a description of what happened. A rule for the future. One sentence, imperative voice.

Example: "Never use inline styles — external stylesheets or scoped CSS only."
Example: "If you don't have the file content, refuse the task instead of hallucinating."

Your principle:"#
    );

    match llm.call(&prompt).await {
        Ok(lesson) => {
            let lesson = lesson.trim().trim_matches('"').to_string();
            tracing::info!(lesson = %lesson, "meta-lesson distilled from failure");

            // Record as a taste signal with high negative weight
            let signal = ww_workspace::TasteSignal {
                score: -1.0,
                reason: format!("[META-LESSON] {}", lesson),
                task_id: "system:lesson".to_string(),
                change_id: None,
                timestamp: now_secs(),
                tags: vec!["meta-lesson".to_string()],
            };
            db.record_taste(&signal).ok();
        }
        Err(e) => tracing::warn!("failed to distill lesson: {e}"),
    }
}

async fn build_contextual_guide(
    db: &Db,
    llm: &LlmClient,
    task: &ww_workspace::Task,
) -> String {
    let all_signals = db.all_signals().unwrap_or_default();
    let embedded_count = db.embedding_count("taste_signal").unwrap_or(0);

    // Always include meta-lessons (they're universal)
    let lessons: Vec<&ww_workspace::TasteSignal> = all_signals
        .iter()
        .filter(|s| s.reason.starts_with("[META-LESSON]"))
        .collect();

    // If we have embeddings, do semantic retrieval for relevant signals
    if embedded_count > 5 {
        let query = format!("{} {}", task.intent, task.why);
        if let Ok(query_vec) = llm.embed(&query).await {
            if let Ok(similar) = db.search_similar("taste_signal", &query_vec, 8) {
                let mut guide = String::new();
                guide.push_str(&format!(
                    "## Taste Guide (semantic — {} most relevant of {} signals)\n\n",
                    similar.len(),
                    all_signals.len()
                ));

                if !lessons.is_empty() {
                    guide.push_str("**Lessons (always apply):**\n");
                    for s in &lessons {
                        guide.push_str(&format!(
                            "- {}\n",
                            s.reason.trim_start_matches("[META-LESSON] ")
                        ));
                    }
                    guide.push('\n');
                }

                guide.push_str("**Relevant signals for this task:**\n");
                for r in &similar {
                    if !r.text.starts_with("[META-LESSON]") {
                        guide.push_str(&format!("- {} (relevance: {:.0}%)\n", r.text, r.score * 100.0));
                    }
                }

                return guide;
            }
        }
    }

    // Fallback: full taste guide
    build_taste_guide(&all_signals)
}

/// Embed a taste signal and store it. Called after recording a new signal.
async fn embed_taste_signal(db: &Db, llm: &LlmClient, signal: &ww_workspace::TasteSignal) {
    let text = &signal.reason;
    match llm.embed(text).await {
        Ok(vec) => {
            let id = format!("taste_{}", signal.timestamp as u64);
            db.store_embedding(&id, "taste_signal", &signal.task_id, text, &vec).ok();
            tracing::debug!("embedded taste signal");
        }
        Err(e) => tracing::debug!("embed failed (non-critical): {e}"),
    }
}

/// Embed a task intent for similar-task lookup.
async fn embed_task(db: &Db, llm: &LlmClient, task: &ww_workspace::Task) {
    let text = format!("{}: {}", task.intent, task.why);
    match llm.embed(&text).await {
        Ok(vec) => {
            db.store_embedding(&task.id, "task", &task.id, &text, &vec).ok();
            tracing::debug!("embedded task {}", task.id);
        }
        Err(e) => tracing::debug!("task embed failed (non-critical): {e}"),
    }
}

fn deploy_site(root: &std::path::Path) {
    let site_dir = root.join("site");
    let dest = std::env::var("WW_DEPLOY_HOST").unwrap_or_default();
    let web_root = std::env::var("WW_DEPLOY_PATH")
        .unwrap_or_else(|_| "/var/www/workwright.xyz/html".to_string());

    if dest.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        let rsync = std::process::Command::new("rsync")
            .args(["-az", "--delete"])
            .arg(format!("{}/", site_dir.display()))
            .arg(format!("{dest}:/tmp/ww-site/"))
            .output();

        match rsync {
            Ok(out) if out.status.success() => {
                let mv = std::process::Command::new("ssh")
                    .args([&dest, &format!("sudo rsync -a /tmp/ww-site/ {web_root}/")])
                    .output();
                match mv {
                    Ok(out) if out.status.success() => tracing::info!("deployed to {dest}"),
                    _ => tracing::warn!("deploy mv failed"),
                }
            }
            _ => tracing::warn!("rsync failed"),
        }
    });
}

async fn get_taste(
    State(state): State<SharedState>,
    axum::extract::Query(params): axum::extract::Query<TasteRawQuery>,
) -> impl IntoResponse {
    match (state.db.all_signals(), state.db.signal_count()) {
        (Ok(signals), Ok(count)) => {
            // ?raw=true returns the raw bullet-point guide
            if params.raw.unwrap_or(false) {
                let guide = build_taste_guide(&signals);
                return Json(serde_json::json!({
                    "text": guide,
                    "signal_count": count,
                }))
                .into_response();
            }

            // Check for cached narrative that matches current signal count
            if let Ok(Some((narrative, cached_count))) = state.db.get_cache("taste_narrative") {
                if cached_count == count {
                    return Json(serde_json::json!({
                        "text": narrative,
                        "signal_count": count,
                    }))
                    .into_response();
                }
            }

            // Cache miss or stale — try to synthesize if LLM is available
            if let Some(llm) = &state.llm {
                match synthesize_taste_narrative(llm, &signals).await {
                    Ok(narrative) => {
                        state.db.set_cache("taste_narrative", &narrative, count).ok();
                        return Json(serde_json::json!({
                            "text": narrative,
                            "signal_count": count,
                        }))
                        .into_response();
                    }
                    Err(e) => {
                        tracing::warn!("failed to synthesize taste narrative: {e}");
                    }
                }
            }

            // Fallback to raw guide
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

#[derive(Deserialize)]
struct TasteRawQuery {
    raw: Option<bool>,
}

#[derive(Deserialize)]
struct TasteSearchQuery {
    q: Option<String>,
}

async fn get_taste_search(
    State(state): State<SharedState>,
    axum::extract::Query(params): axum::extract::Query<TasteSearchQuery>,
) -> impl IntoResponse {
    let query = params.q.unwrap_or_default();
    if query.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "q parameter required"}))).into_response();
    }

    let llm = match &state.llm {
        Some(l) => l,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "embeddings unavailable"}))).into_response(),
    };

    let query_vec = match llm.embed(&query).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let results = match state.db.search_similar("taste_signal", &query_vec, 8) {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|r| serde_json::json!({
            "text": r.text,
            "score": (r.score * 100.0).round() / 100.0,
            "source_id": r.source_id,
        }))
        .collect();

    Json(serde_json::json!({
        "query": query,
        "results": items,
        "total_signals": state.db.signal_count().unwrap_or(0),
    })).into_response()
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

async fn get_diff(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let task = match state.db.get_task(&task_id) {
        Ok(Some(t)) => t,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response(),
    };

    let file_scope = task.scope.split(':').next().unwrap_or(&task.scope);
    let original = state.workspace.read_file(file_scope).unwrap_or_default();
    let staged = state.workspace.staging.read(file_scope)
        .ok()
        .flatten()
        .unwrap_or_default();

    // Compute a simple line diff for the code view
    let changes = if !staged.is_empty() && !original.is_empty() {
        compute_diff(&original, &staged)
    } else {
        String::new()
    };

    Json(serde_json::json!({
        "task_id": task.id,
        "scope": task.scope,
        "file_path": file_scope,
        "original": original,
        "staged": staged,
        "has_staged": !staged.is_empty(),
        "diff": changes,
    })).into_response()
}

async fn get_render(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let task = match state.db.get_task(&task_id) {
        Ok(Some(t)) => t,
        _ => return (StatusCode::NOT_FOUND, "task not found").into_response(),
    };

    let file_scope = task.scope.split(':').next().unwrap_or(&task.scope);

    // Try staged first, fall back to live
    let content = state.workspace.staging.read(file_scope)
        .ok()
        .flatten()
        .or_else(|| state.workspace.read_file(file_scope));

    match content {
        Some(html) if file_scope.ends_with(".html") => {
            axum::response::Html(html).into_response()
        }
        _ => (StatusCode::NOT_FOUND, "no renderable content").into_response(),
    }
}

// --- Helpers ---

/// Synthesize raw taste signals into a narrative style guide via LLM.
async fn synthesize_taste_narrative(
    llm: &LlmClient,
    signals: &[ww_workspace::TasteSignal],
) -> std::result::Result<String, String> {
    if signals.is_empty() {
        return Ok("No taste signals yet. The guide emerges from crit.".to_string());
    }

    // Build raw material for the LLM
    let mut raw = String::new();
    for s in signals {
        let kind = if s.reason.starts_with("[META-LESSON]") {
            "LESSON"
        } else if s.score > 0.0 {
            "ACCEPTED"
        } else {
            "REJECTED"
        };
        raw.push_str(&format!(
            "[{kind}] (score: {:.1}) {}\n",
            s.score,
            s.reason.trim_start_matches("[META-LESSON] ")
        ));
    }

    let prompt = format!(
        r#"You are writing a style guide for a crit-driven development system called Workwright.

Below are {count} raw taste signals — judgments recorded from human crit of AI-generated work. Each is tagged ACCEPTED (positive), REJECTED (negative), or LESSON (distilled from repeated failure).

**Raw signals:**
{raw}

**Your task:** Synthesize these into a narrative style guide. NOT a bullet list. Write it as connected prose that reads like an opinionated design document — the kind of thing a creative director would write for a team.

Structure it with clear sections. Group related signals by theme (architecture, code organization, typography, UX patterns, etc. — whatever themes emerge naturally from the signals). Each section should:
1. State the principle clearly
2. Explain WHY it matters in this context (drawing from the signal reasoning)
3. Give concrete examples of what's accepted vs. rejected

Lessons from failure should be woven in prominently — they represent the hardest-won knowledge.

Keep the voice direct, opinionated, and slightly sharp. This isn't corporate guidelines — it's earned taste. Write like someone who's been through the crit loop and knows what survives.

Use markdown formatting. Start with a brief intro paragraph about what this guide is and how it was generated (from {count} crit signals, not written by hand). Then sections. No bullet-point dumps.

Write the guide:"#,
        count = signals.len(),
        raw = raw,
    );

    llm.call(&prompt).await.map_err(|e| format!("{e}"))
}

fn build_taste_guide(signals: &[ww_workspace::TasteSignal]) -> String {
    if signals.is_empty() {
        return "No taste signals yet. The guide emerges from crit.".to_string();
    }

    let lessons: Vec<_> = signals.iter()
        .filter(|s| s.reason.starts_with("[META-LESSON]"))
        .collect();
    let accepted: Vec<_> = signals.iter()
        .filter(|s| s.score > 0.0)
        .collect();
    let rejected: Vec<_> = signals.iter()
        .filter(|s| s.score <= 0.0 && !s.reason.starts_with("[META-LESSON]"))
        .collect();

    let mut guide = format!(
        "## Taste Guide (learned from human feedback)\n\n*Based on {} taste signals.*\n\n",
        signals.len()
    );

    // Lessons first — these are the hardest-won knowledge
    if !lessons.is_empty() {
        guide.push_str("**Lessons (distilled from repeated failure):**\n");
        for s in &lessons {
            guide.push_str(&format!("- {}\n", s.reason.trim_start_matches("[META-LESSON] ")));
        }
        guide.push('\n');
    }

    if !accepted.is_empty() {
        guide.push_str("**Principles (from accepted work):**\n");
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
    files: &[(String, String)],
    taste_guide: &str,
) -> String {
    let feedback_block = if task.feedback.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = task.feedback.iter()
            .enumerate()
            .map(|(i, f)| format!("  {}. {}", i + 1, f))
            .collect();
        format!(
            "\n## Previous Attempts (rejected)\nAttempt {}. Rejected for:\n{}\nAddress the feedback directly.\n",
            task.attempts + 1,
            items.join("\n")
        )
    };

    let is_multi = files.len() > 1;
    let all_new = files.iter().all(|(_, c)| c.is_empty());

    // Build file content with line numbers for existing files
    let files_block = files.iter().map(|(path, content)| {
        if content.is_empty() {
            format!("### {} (new file — create from scratch)", path)
        } else {
            let numbered: String = content.lines()
                .enumerate()
                .map(|(i, line)| format!("{:4}| {}", i + 1, line))
                .collect::<Vec<_>>()
                .join("\n");
            format!("### {}\n```\n{}\n```", path, numbered)
        }
    }).collect::<Vec<_>>().join("\n\n");

    let instructions = if all_new && !is_multi {
        "Return the complete file content. No explanations, no markdown fences, just the code.".to_string()
    } else if is_multi {
        r#"You may need to create or edit MULTIPLE files. Use this format:

===FILE: path/to/file===
(for existing files, use line-range edits)
(for new files, write the complete content)
===FILE: path/to/other===
...

Edit format for existing files (reference the line numbers shown):
<<<LINES 18-22
replacement content for those lines
>>>END

To INSERT new lines after a specific line:
<<<INSERT_AFTER 17
new lines to add
>>>END

To DELETE lines:
<<<DELETE 18-22>>>

Rules:
- Use the line numbers shown in the file listing
- For new files, just write the complete content after ===FILE: path===
- Do NOT reproduce entire existing files — only the changes"#.to_string()
    } else {
        r#"Return ONLY the changes needed. Reference the line numbers shown in the file listing.

Edit format (use line numbers from the listing above):
<<<LINES 18-22
replacement content for those lines
>>>END

To INSERT new lines after a specific line:
<<<INSERT_AFTER 17
new lines to add
>>>END

To DELETE lines:
<<<DELETE 18-22>>>

Rules:
- Use the exact line numbers shown in the file listing
- Multiple changes: use multiple edit blocks
- Do NOT reproduce the entire file
- Process edits from bottom to top if you have multiple changes (to keep line numbers stable)"#.to_string()
    };

    format!(
        r#"You are a wright — a craftsperson who works within a tradition.

## Taste Guide
{taste_guide}
{feedback_block}
## Your Task
**Intent:** {intent}
**Why:** {why}
**Scope:** {scope}

## Files
{files_block}

## Instructions
{instructions}"#,
        taste_guide = taste_guide,
        feedback_block = feedback_block,
        intent = task.intent,
        why = task.why,
        scope = task.scope,
        files_block = files_block,
        instructions = instructions,
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

/// Parse wright output that may contain changes to multiple files.
/// Supports three formats:
/// 1. `===FILE: path===` blocks with SEARCH/REPLACE or complete content
/// 2. Single-file SEARCH/REPLACE (backward compatible)
/// 3. Complete file content (for new files)
fn apply_edits(original: &str, edit_block: &str) -> std::result::Result<String, String> {
    let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();

    // Collect all edits, then apply from bottom to top (preserves line numbers)
    let mut edits: Vec<(String, usize, usize, Vec<String>)> = Vec::new();

    // Parse <<<LINES start-end ... >>>END blocks
    for block in edit_block.split("<<<LINES ").skip(1) {
        let Some((header, rest)) = block.split_once('\n') else { continue };
        let range = header.trim();
        let Some((start_s, end_s)) = range.split_once('-') else { continue };
        let start: usize = start_s.trim().parse().unwrap_or(0);
        let end: usize = end_s.trim().parse().unwrap_or(0);
        if start == 0 || end == 0 { continue; }

        let content = rest.split(">>>END").next().unwrap_or("").trim_matches('\n');
        let new_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        edits.push(("replace".to_string(), start, end, new_lines));
    }

    // Parse <<<INSERT_AFTER n ... >>>END blocks
    for block in edit_block.split("<<<INSERT_AFTER ").skip(1) {
        let Some((header, rest)) = block.split_once('\n') else { continue };
        let after: usize = header.trim().parse().unwrap_or(0);
        if after == 0 { continue; }

        let content = rest.split(">>>END").next().unwrap_or("").trim_matches('\n');
        let new_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        edits.push(("insert".to_string(), after, after, new_lines));
    }

    // Parse <<<DELETE start-end>>> blocks
    for block in edit_block.split("<<<DELETE ").skip(1) {
        let range = block.split(">>>").next().unwrap_or("").trim();
        let Some((start_s, end_s)) = range.split_once('-') else { continue };
        let start: usize = start_s.trim().parse().unwrap_or(0);
        let end: usize = end_s.trim().parse().unwrap_or(0);
        if start == 0 || end == 0 { continue; }
        edits.push(("delete".to_string(), start, end, Vec::new()));
    }

    if edits.is_empty() {
        return Err("No line-range edits found".to_string());
    }

    // Sort by start line, descending (apply bottom-up to preserve line numbers)
    edits.sort_by(|a, b| b.1.cmp(&a.1));

    for (action, start, end, new_lines) in &edits {
        let s = start.saturating_sub(1); // 1-indexed to 0-indexed
        let len = lines.len();
        let e = (*end).min(len);

        match action.as_str() {
            "replace" => {
                lines.splice(s..e, new_lines.iter().cloned());
            }
            "insert" => {
                let len2 = lines.len();
                let pos = (*start).min(len2);
                for (i, line) in new_lines.iter().enumerate() {
                    lines.insert(pos + i, line.clone());
                }
            }
            "delete" => {
                lines.drain(s..e);
            }
            _ => {}
        }
    }

    Ok(lines.join("\n"))
}

fn apply_file_edits(original: &str, body: &str) -> std::result::Result<String, String> {
    // Try line-range edits first (new format)
    if body.contains("<<<LINES ") || body.contains("<<<INSERT_AFTER ") || body.contains("<<<DELETE ") {
        return apply_edits(original, body);
    }
    // Fall back to SEARCH/REPLACE (old format)
    if body.contains("<<<SEARCH") {
        return apply_diff(original, body);
    }
    Err("No recognized edit format".to_string())
}

fn parse_multi_file_output(
    output: &str,
    original_files: &[(String, String)],
) -> Vec<(String, String)> {
    let mut results = Vec::new();

    if output.contains("===FILE:") {
        let blocks: Vec<&str> = output.split("===FILE:").collect();
        for block in blocks.iter().skip(1) {
            let Some((header, body)) = block.split_once("===") else { continue };
            let path = header.trim().to_string();
            let body = body.trim();

            let original = original_files
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, c)| c.as_str())
                .unwrap_or("");

            let content = if original.is_empty() {
                strip_fences(body)
            } else {
                match apply_file_edits(original, body) {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::warn!(path, error = %e, "edit failed for file");
                        continue;
                    }
                }
            };

            results.push((path, content));
        }
    } else if original_files.len() == 1 {
        let (path, original) = &original_files[0];

        let content = if original.is_empty() {
            strip_fences(output)
        } else {
            match apply_file_edits(original, output) {
                Ok(result) => result,
                Err(_) => strip_fences(output),
            }
        };

        results.push((path.clone(), content));
    }

    results
}

fn compute_diff(original: &str, staged: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let staged_lines: Vec<&str> = staged.lines().collect();
    let mut output = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < orig_lines.len() || j < staged_lines.len() {
        if i < orig_lines.len() && j < staged_lines.len() && orig_lines[i] == staged_lines[j] {
            i += 1;
            j += 1;
        } else {
            // Found a difference — collect context + changed lines
            let ctx_start = i.saturating_sub(2);
            for k in ctx_start..i {
                output.push(format!("  {}", orig_lines[k]));
            }

            // Collect removed lines
            let mut orig_end = i;
            while orig_end < orig_lines.len() {
                if j < staged_lines.len() && orig_lines.get(orig_end) == staged_lines.get(j) {
                    break;
                }
                // Check if this line appears soon in staged (added, not removed)
                let in_staged = staged_lines[j..].iter().take(20).any(|&l| l == orig_lines[orig_end]);
                if in_staged { break; }
                output.push(format!("- {}", orig_lines[orig_end]));
                orig_end += 1;
            }

            // Collect added lines
            while j < staged_lines.len() {
                if orig_end < orig_lines.len() && staged_lines[j] == orig_lines[orig_end] {
                    break;
                }
                output.push(format!("+ {}", staged_lines[j]));
                j += 1;
            }

            // Context after
            let ctx_end = orig_end.min(orig_lines.len()).saturating_add(2).min(orig_lines.len());
            for k in orig_end..ctx_end {
                if k < orig_lines.len() {
                    output.push(format!("  {}", orig_lines[k]));
                }
            }

            if !output.is_empty() && output.last().map(|l| l.as_str()) != Some("---") {
                output.push("---".to_string());
            }

            i = orig_end;
        }
    }

    output.join("\n")
}

fn apply_diff(original: &str, diff_output: &str) -> std::result::Result<String, String> {
    let mut result = original.to_string();
    let mut applied = 0;

    // Parse SEARCH/REPLACE blocks
    let blocks: Vec<&str> = diff_output.split("<<<SEARCH").collect();
    for block in blocks.iter().skip(1) {
        let parts: Vec<&str> = block.splitn(2, ">>>REPLACE").collect();
        if parts.len() != 2 {
            return Err(format!("Malformed block — missing >>>REPLACE"));
        }

        let search = parts[0].trim_matches('\n');

        let replace_and_rest: Vec<&str> = parts[1].splitn(2, "<<<END").collect();
        let replace = replace_and_rest[0].trim_matches('\n');

        // Try exact match first
        if result.contains(search) {
            result = result.replacen(search, replace, 1);
            applied += 1;
        } else {
            // Try with trimmed whitespace matching (fuzzy)
            let search_trimmed: Vec<&str> = search.lines()
                .map(|l| l.trim())
                .collect();
            let result_lines: Vec<String> = result.lines()
                .map(|l| l.to_string())
                .collect();

            let mut found = false;
            'outer: for i in 0..result_lines.len() {
                if i + search_trimmed.len() > result_lines.len() { break; }
                for (j, search_line) in search_trimmed.iter().enumerate() {
                    if result_lines[i + j].trim() != *search_line {
                        continue 'outer;
                    }
                }
                // Found fuzzy match at line i
                let mut new_lines: Vec<String> = result_lines[..i].to_vec();
                new_lines.extend(replace.lines().map(|l| l.to_string()));
                new_lines.extend(result_lines[i + search_trimmed.len()..].to_vec());
                result = new_lines.join("\n");
                applied += 1;
                found = true;
                break;
            }
            if !found {
                return Err(format!(
                    "Could not find SEARCH block in file: '{}'",
                    &search[..search.len().min(80)]
                ));
            }
        }
    }

    if applied == 0 {
        return Err("No SEARCH/REPLACE blocks found".to_string());
    }

    Ok(result)
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
    feedback: Vec<String>,
    attempts: u32,
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
            feedback: t.feedback,
            attempts: t.attempts,
        }
    }
}
