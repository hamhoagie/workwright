//! Workspace errors — typed, not stringly.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error("file locked by {holder}: {intent}")]
    Locked { path: String, holder: String, intent: String },

    #[error("task {0} is {1}, not claimable")]
    NotClaimable(String, String),

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;
