//! Changelog — every change recorded with who, what, why.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub path: String,
    pub intent: String,
    pub agent_id: String,
    pub timestamp: f64,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taste_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taste_note: Option<String>,
}

pub struct Changelog {
    file: std::path::PathBuf,
}

impl Changelog {
    pub fn new(meta_dir: &Path) -> Self {
        let file = meta_dir.join("changelog.jsonl");
        if !file.exists() {
            std::fs::File::create(&file).ok();
        }
        Self { file }
    }

    pub fn record(
        &self,
        path: &str,
        agent_id: &str,
        intent: &str,
        before: Option<&str>,
        after: Option<&str>,
    ) -> Result<Change> {
        let now = now_secs();
        let id = {
            let mut hasher = Sha256::new();
            hasher.update(format!("{}:{}:{}", path, now, agent_id));
            hex::encode(&hasher.finalize()[..6])
        };

        let change = Change {
            id,
            path: path.to_string(),
            intent: intent.to_string(),
            agent_id: agent_id.to_string(),
            timestamp: now,
            before_hash: before.map(hash_content),
            after_hash: after.map(hash_content),
            taste_score: None,
            taste_note: None,
        };

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        use std::io::Write;
        writeln!(file, "{}", serde_json::to_string(&change)?)?;

        Ok(change)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<Change>> {
        let text = std::fs::read_to_string(&self.file)?;
        let mut changes: Vec<Change> = text
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        changes.reverse();
        changes.truncate(limit);
        Ok(changes)
    }

    pub fn get(&self, change_id: &str) -> Result<Option<Change>> {
        let text = std::fs::read_to_string(&self.file)?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str::<Change>(l).ok())
            .find(|c| c.id == change_id))
    }
}

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
