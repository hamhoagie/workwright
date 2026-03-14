//! SQLite storage — the single source of truth.
//!
//! Schema mirrors the JSONL format exactly.
//! WAL mode for concurrent reads during wright execution.
//! One file: `.workwright/workwright.db`

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::task::{Task, TaskStatus};
use crate::taste::TasteSignal;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id            TEXT PRIMARY KEY,
    intent        TEXT NOT NULL,
    why           TEXT NOT NULL,
    scope         TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    created       REAL NOT NULL,
    agent_id      TEXT,
    defense       TEXT,
    context       TEXT DEFAULT '[]',
    change_ids    TEXT DEFAULT '[]',
    taste_score   REAL,
    taste_note    TEXT,
    submitted_by      TEXT,
    submitted_by_name TEXT,
    critted_by        TEXT,
    critted_by_name   TEXT
);

CREATE TABLE IF NOT EXISTS taste_signals (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    score     REAL NOT NULL,
    reason    TEXT NOT NULL,
    task_id   TEXT NOT NULL,
    change_id TEXT,
    timestamp REAL NOT NULL,
    tags      TEXT DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    token         TEXT NOT NULL UNIQUE,
    trust_score   REAL NOT NULL DEFAULT 0.0,
    role          TEXT NOT NULL DEFAULT 'participant',
    created       REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS changes (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    intent      TEXT NOT NULL,
    agent_id    TEXT NOT NULL,
    timestamp   REAL NOT NULL,
    before_hash TEXT,
    after_hash  TEXT,
    taste_score REAL,
    taste_note  TEXT
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created);
CREATE INDEX IF NOT EXISTS idx_users_token ON users(token);
CREATE INDEX IF NOT EXISTS idx_taste_task ON taste_signals(task_id);
"#;

/// Thread-safe database handle.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(meta_dir: &Path) -> Result<Self> {
        let path = meta_dir.join("workwright.db");
        let conn = Connection::open(&path)?;

        // WAL mode — concurrent reads while wright writes
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // --- Tasks ---

    pub fn create_task(&self, task: &Task) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tasks (id, intent, why, scope, status, created, agent_id, defense, \
             context, change_ids, taste_score, taste_note, submitted_by, submitted_by_name, \
             critted_by, critted_by_name) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                task.id,
                task.intent,
                task.why,
                task.scope,
                task.status.to_string(),
                task.created,
                task.agent_id,
                task.defense,
                serde_json::to_string(&task.context).unwrap_or_default(),
                serde_json::to_string(&task.change_ids).unwrap_or_default(),
                task.taste_score,
                task.taste_note,
                task.submitted_by,
                task.submitted_by_name,
                task.critted_by,
                task.critted_by_name,
            ],
        )?;
        Ok(())
    }

    pub fn all_tasks(&self) -> Result<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, intent, why, scope, status, created, agent_id, defense, \
             context, change_ids, taste_score, taste_note, submitted_by, submitted_by_name, \
             critted_by, critted_by_name \
             FROM tasks ORDER BY created DESC",
        )?;
        let tasks = stmt
            .query_map([], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    intent: row.get(1)?,
                    why: row.get(2)?,
                    scope: row.get(3)?,
                    status: parse_status(&row.get::<_, String>(4)?),
                    created: row.get(5)?,
                    agent_id: row.get(6)?,
                    defense: row.get(7)?,
                    context: parse_json_vec(row.get::<_, Option<String>>(8)?),
                    change_ids: parse_json_vec(row.get::<_, Option<String>>(9)?),
                    taste_score: row.get(10)?,
                    taste_note: row.get(11)?,
                    submitted_by: row.get(12)?,
                    submitted_by_name: row.get(13)?,
                    critted_by: row.get(14)?,
                    critted_by_name: row.get(15)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tasks)
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        Ok(self.all_tasks()?.into_iter().find(|t| t.id == id))
    }

    pub fn update_task(&self, task: &Task) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET status=?1, agent_id=?2, defense=?3, change_ids=?4, \
             taste_score=?5, taste_note=?6, submitted_by=?7, submitted_by_name=?8, \
             critted_by=?9, critted_by_name=?10 WHERE id=?11",
            params![
                task.status.to_string(),
                task.agent_id,
                task.defense,
                serde_json::to_string(&task.change_ids).unwrap_or_default(),
                task.taste_score,
                task.taste_note,
                task.submitted_by,
                task.submitted_by_name,
                task.critted_by,
                task.critted_by_name,
                task.id,
            ],
        )?;
        Ok(())
    }

    // --- Taste ---

    pub fn record_taste(&self, signal: &TasteSignal) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO taste_signals (score, reason, task_id, change_id, timestamp, tags) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                signal.score,
                signal.reason,
                signal.task_id,
                signal.change_id,
                signal.timestamp,
                serde_json::to_string(&signal.tags).unwrap_or_default(),
            ],
        )?;
        Ok(())
    }

    pub fn all_signals(&self) -> Result<Vec<TasteSignal>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT score, reason, task_id, change_id, timestamp, tags \
             FROM taste_signals ORDER BY timestamp",
        )?;
        let signals = stmt
            .query_map([], |row| {
                Ok(TasteSignal {
                    score: row.get(0)?,
                    reason: row.get(1)?,
                    task_id: row.get(2)?,
                    change_id: row.get(3)?,
                    timestamp: row.get(4)?,
                    tags: parse_json_vec(row.get::<_, Option<String>>(5)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(signals)
    }

    pub fn signal_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM taste_signals", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    // --- Users ---

    pub fn create_user(&self, user: &User) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (id, email, display_name, token, trust_score, role, created) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                user.id,
                user.email,
                user.display_name,
                user.token,
                user.trust_score,
                user.role,
                user.created,
            ],
        )?;
        Ok(())
    }

    pub fn get_user_by_token(&self, token: &str) -> Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, display_name, token, trust_score, role, created \
             FROM users WHERE token = ?1",
        )?;
        let user = stmt
            .query_row(params![token], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    token: row.get(3)?,
                    trust_score: row.get(4)?,
                    role: row.get(5)?,
                    created: row.get(6)?,
                })
            })
            .ok();
        Ok(user)
    }

    pub fn all_users(&self) -> Result<Vec<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, display_name, token, trust_score, role, created \
             FROM users ORDER BY created",
        )?;
        let users = stmt
            .query_map([], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    token: row.get(3)?,
                    trust_score: row.get(4)?,
                    role: row.get(5)?,
                    created: row.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(users)
    }

    pub fn update_trust(&self, user_id: &str, delta: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET trust_score = MIN(1.0, MAX(0.0, trust_score + ?1)) WHERE id = ?2",
            params![delta, user_id],
        )?;
        Ok(())
    }

    // --- Migration ---

    /// Import existing JSONL data into SQLite.
    /// Idempotent — skips rows that already exist.
    pub fn migrate_from_jsonl(&self, meta_dir: &Path) -> Result<MigrateStats> {
        let mut stats = MigrateStats::default();

        // Tasks
        let tasks_file = meta_dir.join("tasks.jsonl");
        if tasks_file.exists() {
            let text = std::fs::read_to_string(&tasks_file)?;
            let mut seen = std::collections::HashMap::new();
            for line in text.lines().filter(|l| !l.is_empty()) {
                if let Ok(task) = serde_json::from_str::<Task>(line) {
                    seen.insert(task.id.clone(), task);
                }
            }
            for task in seen.values() {
                if self.get_task(&task.id)?.is_none() {
                    self.create_task(task)?;
                    stats.tasks += 1;
                }
            }
        }

        // Taste signals
        let taste_file = meta_dir.join("taste.jsonl");
        if taste_file.exists() {
            let text = std::fs::read_to_string(&taste_file)?;
            let existing = self.signal_count()?;
            let signals: Vec<TasteSignal> = text
                .lines()
                .filter(|l| !l.is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            // Only import if we have more in JSONL than DB
            if signals.len() > existing {
                for signal in &signals[existing..] {
                    self.record_taste(signal)?;
                    stats.signals += 1;
                }
            }
        }

        // Users
        let users_file = meta_dir.join("users.jsonl");
        if users_file.exists() {
            let text = std::fs::read_to_string(&users_file)?;
            for line in text.lines().filter(|l| !l.is_empty()) {
                if let Ok(user) = serde_json::from_str::<User>(line) {
                    if self.get_user_by_token(&user.token)?.is_none() {
                        self.create_user(&user)?;
                        stats.users += 1;
                    }
                }
            }
        }

        Ok(stats)
    }
}

#[derive(Debug, Default)]
pub struct MigrateStats {
    pub tasks: usize,
    pub signals: usize,
    pub users: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub token: String,
    pub trust_score: f64,
    pub role: String,
    pub created: f64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

fn parse_status(s: &str) -> TaskStatus {
    match s {
        "pending" => TaskStatus::Pending,
        "active" => TaskStatus::Active,
        "review" => TaskStatus::Review,
        "accepted" => TaskStatus::Accepted,
        "rejected" => TaskStatus::Rejected,
        "failed" => TaskStatus::Failed,
        _ => TaskStatus::Pending,
    }
}

fn parse_json_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
