//! Staging — wrights write here, not to live files.
//!
//! The trust gradient applied to the filesystem:
//! wrights can propose, only accepted crit deploys.

use std::path::{Path, PathBuf};

use crate::error::Result;

pub struct StagingArea {
    dir: PathBuf,
}

impl StagingArea {
    pub fn new(meta_dir: &Path) -> Self {
        let dir = meta_dir.join("staging");
        std::fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    /// Write content to staging. Live file stays untouched.
    pub fn write(&self, path: &str, content: &str) -> Result<()> {
        let staged = self.dir.join(path);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&staged, content)?;
        Ok(())
    }

    /// Read staged content (for preview).
    pub fn read(&self, path: &str) -> Result<Option<String>> {
        let staged = self.dir.join(path);
        if staged.exists() {
            Ok(Some(std::fs::read_to_string(&staged)?))
        } else {
            Ok(None)
        }
    }

    /// Promote staged → live. Returns true if promoted.
    pub fn promote(&self, path: &str, root: &Path) -> Result<bool> {
        let staged = self.dir.join(path);
        if !staged.exists() {
            return Ok(false);
        }
        let live = root.join(path);
        if let Some(parent) = live.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&staged, &live)?;
        std::fs::remove_file(&staged)?;
        Ok(true)
    }

    /// Discard staged content (on rejection).
    pub fn discard(&self, path: &str) {
        let staged = self.dir.join(path);
        std::fs::remove_file(staged).ok();
    }
}
