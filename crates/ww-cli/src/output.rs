//! Pretty-printing for CLI output.

use serde::Deserialize;

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct TaskJson {
    pub id: String,
    pub intent: String,
    pub why: String,
    pub scope: String,
    pub status: String,
    pub created: f64,
    pub agent_id: Option<String>,
    pub defense: Option<String>,
    pub change_ids: Vec<String>,
    pub taste_score: Option<f64>,
    pub taste_note: Option<String>,
    pub submitted_by: Option<String>,
    pub submitted_by_name: Option<String>,
    pub feedback: Vec<String>,
    pub attempts: u32,
}

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct TasteJson {
    pub text: String,
    pub signal_count: usize,
}

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct UserJson {
    pub id: String,
    pub display_name: String,
    pub trust_score: f64,
    pub role: String,
}

// --- Printers ---

pub fn print_task_table(tasks: &[&TaskJson]) {
    if tasks.is_empty() {
        println!("No tasks found.");
        return;
    }

    let mut w_id = 2;
    let mut w_status = 6;
    for t in tasks {
        w_id = w_id.max(t.id.len());
        w_status = w_status.max(t.status.len());
    }

    println!(
        "{:<w_id$}  {:<w_status$}  {:>5}  {}",
        "ID", "STATUS", "SCORE", "INTENT",
        w_id = w_id,
        w_status = w_status,
    );
    println!("{}", "-".repeat(w_id + w_status + 60));

    for t in tasks {
        let score = t
            .taste_score
            .map(|s| format!("{:+.2}", s))
            .unwrap_or_else(|| "  —".into());
        let intent = truncate(&t.intent, 60);
        println!(
            "{:<w_id$}  {:<w_status$}  {:>5}  {}",
            t.id,
            t.status,
            score,
            intent,
            w_id = w_id,
            w_status = w_status,
        );
    }

    println!("\n{} task(s)", tasks.len());
}

pub fn print_task_created(task: &TaskJson) {
    println!("✓ Task created: {}", task.id);
    println!("  intent: {}", task.intent);
    println!("  scope:  {}", task.scope);
    println!("  A wright is picking this up. Watch with `ww tasks --status review`.");
}

pub fn print_crit_result(resp: &serde_json::Value) {
    let score = resp.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
    let retrying = resp
        .get("retrying")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    let attempt = resp.get("attempt").and_then(|a| a.as_u64()).unwrap_or(0);

    if score > 0.0 {
        println!("✓ Accepted. Deployed to live.");
    } else if retrying {
        println!("↻ Rejected. Wright is retrying (attempt {}/3).", attempt + 1);
    } else {
        println!("✗ Closed after {} attempts. Meta-lesson distilled.", attempt);
    }
}

pub fn print_taste_search(resp: &serde_json::Value) {
    let results = resp
        .get("results")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let total = resp.get("total_signals").and_then(|t| t.as_u64()).unwrap_or(0);
    let query = resp.get("query").and_then(|q| q.as_str()).unwrap_or("");

    println!("Query: \"{query}\" — {} relevant signals (of {total})\n", results.len());
    for r in results {
        let pct = (r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) * 100.0).round();
        let text = r.get("text").and_then(|t| t.as_str()).unwrap_or("");
        println!("  [{:>3.0}%] {}\n", pct, text);
    }
}

pub fn print_status(tasks: &[TaskJson], users: &[UserJson], signal_count: usize) {
    let mut counts = std::collections::HashMap::new();
    for t in tasks {
        *counts.entry(t.status.as_str()).or_insert(0) += 1;
    }

    println!("Workwright status");
    println!("=================\n");
    println!("Tasks:    {} total", tasks.len());
    for (status, n) in &counts {
        println!("  {status}: {n}");
    }
    println!("Signals:  {}", signal_count);
    println!("Users:    {}", users.len());
    println!();
    if let Some(admin) = users.iter().find(|u| u.role == "admin") {
        println!("Admin:    {} (trust {:.2})", admin.display_name, admin.trust_score);
    }
}

pub fn print_users(users: &[UserJson]) {
    if users.is_empty() {
        println!("No users.");
        return;
    }
    println!("{:<10}  {:<12}  {:>6}  ROLE", "ID", "NAME", "TRUST");
    println!("{}", "-".repeat(44));
    for u in users {
        println!(
            "{:<10}  {:<12}  {:>+6.2}  {}",
            u.id, truncate(&u.display_name, 12), u.trust_score, u.role
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
