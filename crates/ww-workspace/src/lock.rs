//! File locking — fine-grained, with expiry.
//!
//! A participant locks the files they're working on.
//! Locks expire after TTL to prevent deadlocks from crashed wrights.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WorkspaceError};

const DEFAULT_TTL_SECS: f64 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lock {
    pub path: String,
    pub agent_id: String,
    pub intent: String,
    pub acquired: f64,
    pub expires: f64,
}

#[derive(Debug)]
pub struct LockManager {
    file: std::path::PathBuf,
}

impl LockManager {
    pub fn new(meta_dir: &Path) -> Self {
        let file = meta_dir.join("locks.json");
        if !file.exists() {
            std::fs::write(&file, "{}").ok();
        }
        Self { file }
    }

    pub fn acquire(&self, path: &str, agent_id: &str, intent: &str) -> Result<Lock> {
        let mut locks = self.read()?;
        self.expire_stale(&mut locks);

        if let Some(existing) = locks.get(path) {
            return Err(WorkspaceError::Locked {
                path: path.to_string(),
                holder: existing.agent_id.clone(),
                intent: existing.intent.clone(),
            });
        }

        let now = now_secs();
        let lock = Lock {
            path: path.to_string(),
            agent_id: agent_id.to_string(),
            intent: intent.to_string(),
            acquired: now,
            expires: now + DEFAULT_TTL_SECS,
        };
        locks.insert(path.to_string(), lock.clone());
        self.write(&locks)?;
        Ok(lock)
    }

    pub fn release(&self, path: &str, agent_id: &str) -> Result<()> {
        let mut locks = self.read()?;
        if let Some(lock) = locks.get(path) {
            if lock.agent_id == agent_id {
                locks.remove(path);
                self.write(&locks)?;
            }
        }
        Ok(())
    }

    pub fn holder(&self, path: &str) -> Result<Option<Lock>> {
        let mut locks = self.read()?;
        self.expire_stale(&mut locks);
        Ok(locks.get(path).cloned())
    }

    pub fn all(&self) -> Result<HashMap<String, Lock>> {
        let mut locks = self.read()?;
        self.expire_stale(&mut locks);
        Ok(locks)
    }

    fn read(&self) -> Result<HashMap<String, Lock>> {
        let text = std::fs::read_to_string(&self.file)?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    fn write(&self, locks: &HashMap<String, Lock>) -> Result<()> {
        let text = serde_json::to_string_pretty(locks)?;
        std::fs::write(&self.file, text)?;
        Ok(())
    }

    fn expire_stale(&self, locks: &mut HashMap<String, Lock>) {
        let now = now_secs();
        locks.retain(|_, v| v.expires > now);
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
