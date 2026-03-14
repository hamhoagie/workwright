//! Workspace — the unified interface.
//!
//! Composes locks, changelog, staging, and file ops
//! into a single coherent surface.

use std::path::{Path, PathBuf};

use crate::change::Changelog;
use crate::error::{Result, WorkspaceError};
use crate::lock::LockManager;
use crate::staging::StagingArea;

pub struct Workspace {
    root: PathBuf,
    pub locks: LockManager,
    pub changelog: Changelog,
    pub staging: StagingArea,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let meta_dir = root.join(".workwright");
        std::fs::create_dir_all(&meta_dir).ok();

        Self {
            locks: LockManager::new(&meta_dir),
            changelog: Changelog::new(&meta_dir),
            staging: StagingArea::new(&meta_dir),
            root,
        }
    }

    /// Read a file from the workspace.
    pub fn read_file(&self, path: &str) -> Option<String> {
        let fp = self.root.join(path);
        std::fs::read_to_string(fp).ok()
    }

    /// Write to staging. Live file stays untouched.
    /// Must hold the lock.
    pub fn write_staged(
        &self,
        path: &str,
        content: &str,
        agent_id: &str,
        intent: &str,
    ) -> Result<()> {
        // Verify lock
        if let Some(lock) = self.locks.holder(path)? {
            if lock.agent_id != agent_id {
                return Err(WorkspaceError::Locked {
                    path: path.to_string(),
                    holder: lock.agent_id,
                    intent: lock.intent,
                });
            }
        }

        let before = self.read_file(path);
        self.staging.write(path, content)?;
        self.changelog.record(
            path,
            agent_id,
            intent,
            before.as_deref(),
            Some(content),
        )?;
        Ok(())
    }

    /// Write directly to live file (for non-wright operations).
    pub fn write_file(
        &self,
        path: &str,
        content: &str,
        agent_id: &str,
        intent: &str,
    ) -> Result<()> {
        let fp = self.root.join(path);
        let before = std::fs::read_to_string(&fp).ok();
        if let Some(parent) = fp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&fp, content)?;
        self.changelog.record(
            path,
            agent_id,
            intent,
            before.as_deref(),
            Some(content),
        )?;
        Ok(())
    }

    /// Promote staged → live on acceptance.
    pub fn promote(&self, path: &str) -> Result<bool> {
        self.staging.promote(path, &self.root)
    }

    /// Discard staged content on rejection.
    pub fn discard(&self, path: &str) {
        self.staging.discard(path);
    }

    /// Root path of the workspace.
    pub fn root(&self) -> &Path {
        &self.root
    }
}
