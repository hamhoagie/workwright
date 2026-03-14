//! Tasks — the atomic unit. One intent, one scope, one why.

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Result, WorkspaceError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Active,
    Review,
    Accepted,
    Rejected,
    Failed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Active => write!(f, "active"),
            Self::Review => write!(f, "review"),
            Self::Accepted => write!(f, "accepted"),
            Self::Rejected => write!(f, "rejected"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub intent: String,
    pub why: String,
    pub scope: String,
    pub status: TaskStatus,
    pub created: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defense: Option<String>,
    #[serde(default)]
    pub context: Vec<String>,
    #[serde(default)]
    pub change_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taste_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taste_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_by_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critted_by_name: Option<String>,
    /// Previous rejection reasons — the wright reads these to improve.
    #[serde(default)]
    pub feedback: Vec<String>,
    /// How many times the wright has attempted this task.
    #[serde(default)]
    pub attempts: u32,
}

pub struct TaskStore {
    file: std::path::PathBuf,
}

impl TaskStore {
    pub fn new(meta_dir: &Path) -> Self {
        let file = meta_dir.join("tasks.jsonl");
        if !file.exists() {
            std::fs::File::create(&file).ok();
        }
        Self { file }
    }

    pub fn create(
        &self,
        intent: &str,
        why: &str,
        scope: &str,
        context: Vec<String>,
    ) -> Result<Task> {
        let task = Task {
            id: Uuid::new_v4().to_string()[..8].to_string(),
            intent: intent.to_string(),
            why: why.to_string(),
            scope: scope.to_string(),
            status: TaskStatus::Pending,
            created: now_secs(),
            agent_id: None,
            defense: None,
            context,
            change_ids: Vec::new(),
            taste_score: None,
            taste_note: None,
            submitted_by: None,
            submitted_by_name: None,
            critted_by: None,
            critted_by_name: None,
            feedback: Vec::new(),
            attempts: 0,
        };
        self.append(&task)?;
        Ok(task)
    }

    pub fn all(&self) -> Result<Vec<Task>> {
        let text = std::fs::read_to_string(&self.file)?;
        // JSONL may have duplicate IDs — last write wins
        let mut seen = std::collections::HashMap::new();
        for line in text.lines().filter(|l| !l.is_empty()) {
            if let Ok(task) = serde_json::from_str::<Task>(line) {
                seen.insert(task.id.clone(), task);
            }
        }
        let mut tasks: Vec<Task> = seen.into_values().collect();
        tasks.sort_by(|a, b| b.created.partial_cmp(&a.created).unwrap());
        Ok(tasks)
    }

    pub fn get(&self, id: &str) -> Result<Option<Task>> {
        Ok(self.all()?.into_iter().find(|t| t.id == id))
    }

    pub fn pending(&self) -> Result<Vec<Task>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .collect())
    }

    pub fn in_review(&self) -> Result<Vec<Task>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|t| t.status == TaskStatus::Review)
            .collect())
    }

    pub fn claim(&self, id: &str, agent_id: &str) -> Result<Task> {
        let mut task = self
            .get(id)?
            .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?;

        if task.status != TaskStatus::Pending {
            return Err(WorkspaceError::NotClaimable(
                id.to_string(),
                task.status.to_string(),
            ));
        }

        task.status = TaskStatus::Active;
        task.agent_id = Some(agent_id.to_string());
        self.append(&task)?;
        Ok(task)
    }

    pub fn submit(&self, id: &str, change_ids: Vec<String>) -> Result<Task> {
        let mut task = self
            .get(id)?
            .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?;
        task.status = TaskStatus::Review;
        task.change_ids = change_ids;
        self.append(&task)?;
        Ok(task)
    }

    pub fn crit(&self, id: &str, score: f64, reason: &str) -> Result<Task> {
        let mut task = self
            .get(id)?
            .ok_or_else(|| WorkspaceError::TaskNotFound(id.to_string()))?;
        task.status = if score > 0.0 {
            TaskStatus::Accepted
        } else {
            TaskStatus::Rejected
        };
        task.taste_score = Some(score);
        task.taste_note = Some(reason.to_string());
        self.append(&task)?;
        Ok(task)
    }

    pub fn update(&self, task: &Task) -> Result<()> {
        self.append(task)
    }

    fn append(&self, task: &Task) -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        use std::io::Write;
        writeln!(file, "{}", serde_json::to_string(task)?)?;
        Ok(())
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
