//! CLI argument definitions.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ww", version, about = "Workwright CLI — brief, crit, taste.", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List tasks, optionally filtered by status
    Tasks(TasksArgs),
    /// Create a new task (brief)
    Task(TaskArgs),
    /// Submit a crit (score + reason) for a task
    Crit(CritArgs),
    /// Show the generated taste guide
    Taste(TasteArgs),
    /// Show workspace overview
    Status,
    /// Show current authenticated user
    Me,
    /// List all users
    Users,
}

#[derive(clap::Args)]
pub struct TasksArgs {
    /// Filter by status: pending, active, review, accepted, rejected, failed
    #[arg(short, long)]
    pub status: Option<String>,
    /// Limit results to N most recent
    #[arg(short, long, default_value = "20")]
    pub limit: usize,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct TaskArgs {
    /// What to build (the intent)
    pub intent: String,
    /// Why it matters (required — the defense depends on this)
    #[arg(short, long)]
    pub why: String,
    /// File scope (default: site/index.html)
    #[arg(short, long)]
    pub scope: Option<String>,
}

#[derive(clap::Args)]
pub struct CritArgs {
    /// Task ID to crit
    pub task_id: String,
    /// Score from -1.0 to 1.0
    #[arg(short, long, allow_negative_numbers = true)]
    pub score: f64,
    /// Why this score — the taste signal
    #[arg(short, long)]
    pub reason: String,
}

#[derive(clap::Args)]
pub struct TasteArgs {
    /// Return the raw bullet-point guide instead of prose narrative
    #[arg(long)]
    pub raw: bool,
    /// Search taste memory semantically
    #[arg(long)]
    pub query: Option<String>,
}
