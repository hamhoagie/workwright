//! Wright — the one who does the work.
//!
//! Picks up a task, reads the taste guide, calls the LLM,
//! writes to staging, defends its choices.
//!
//! Not an "agent." A wright. One who works within a craft tradition.

pub mod llm;
pub mod wright;

pub use wright::Wright;
