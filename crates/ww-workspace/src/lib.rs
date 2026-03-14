//! Workwright workspace — the shared surface where work happens.
//!
//! Files, locks, staging, changelog, tasks, taste.
//! Everything persists to `.workwright/` as JSONL — same format
//! the Python version reads, so migration is seamless.

pub mod change;
pub mod db;
pub mod error;
pub mod lock;
pub mod staging;
pub mod task;
pub mod taste;
pub mod workspace;

pub use db::{Db, User};
pub use error::WorkspaceError;
pub use task::{Task, TaskStatus, TaskStore};
pub use taste::{TasteSignal, TasteStore};
pub use workspace::Workspace;
