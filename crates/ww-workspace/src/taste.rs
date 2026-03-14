//! Taste store — accumulated judgment from every crit.
//!
//! Not a style guide. Not linting rules. A model that learns
//! from human crit: what gets accepted, what gets rejected, and why.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteSignal {
    pub score: f64,
    pub reason: String,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_id: Option<String>,
    pub timestamp: f64,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub struct TasteStore {
    file: std::path::PathBuf,
    patterns_file: std::path::PathBuf,
}

impl TasteStore {
    pub fn new(meta_dir: &Path) -> Self {
        let file = meta_dir.join("taste.jsonl");
        let patterns_file = meta_dir.join("taste_patterns.json");
        if !file.exists() {
            std::fs::File::create(&file).ok();
        }
        Self { file, patterns_file }
    }

    pub fn record(
        &self,
        score: f64,
        reason: &str,
        task_id: &str,
        change_id: Option<&str>,
    ) -> Result<TasteSignal> {
        let signal = TasteSignal {
            score,
            reason: reason.to_string(),
            task_id: task_id.to_string(),
            change_id: change_id.map(|s| s.to_string()),
            timestamp: now_secs(),
            tags: Vec::new(),
        };

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        use std::io::Write;
        writeln!(file, "{}", serde_json::to_string(&signal)?)?;

        self.rebuild_patterns()?;
        Ok(signal)
    }

    pub fn signals(&self) -> Result<Vec<TasteSignal>> {
        let text = std::fs::read_to_string(&self.file)?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }

    pub fn guide(&self) -> Result<String> {
        let signals = self.signals()?;
        if signals.is_empty() {
            return Ok("No taste signals yet. The guide emerges from crit.".to_string());
        }

        let accepted: Vec<_> = signals.iter().filter(|s| s.score > 0.0).collect();
        let rejected: Vec<_> = signals.iter().filter(|s| s.score <= 0.0).collect();

        let mut guide = format!(
            "## Taste Guide (learned from human feedback)\n\n\
             *Based on {} taste signals.*\n\n",
            signals.len()
        );

        if !accepted.is_empty() {
            guide.push_str("**Extracted principles (from accepted work):**\n");
            for s in &accepted {
                guide.push_str(&format!("- {}\n", s.reason));
            }
            guide.push('\n');
        }

        if !rejected.is_empty() {
            guide.push_str("**Anti-patterns (from rejected work):**\n");
            for s in &rejected {
                guide.push_str(&format!("- {}\n", s.reason));
            }
        }

        Ok(guide)
    }

    pub fn patterns(&self) -> Result<TastePatterns> {
        if self.patterns_file.exists() {
            let text = std::fs::read_to_string(&self.patterns_file)?;
            Ok(serde_json::from_str(&text).unwrap_or_default())
        } else {
            Ok(TastePatterns::default())
        }
    }

    fn rebuild_patterns(&self) -> Result<()> {
        let signals = self.signals()?;
        let mut likes: HashMap<String, usize> = HashMap::new();
        let mut dislikes: HashMap<String, usize> = HashMap::new();

        for s in &signals {
            let bucket = if s.score > 0.0 { &mut likes } else { &mut dislikes };
            *bucket.entry("general".to_string()).or_insert(0) += 1;
        }

        let patterns = TastePatterns {
            signal_count: signals.len(),
            likes,
            dislikes,
        };
        std::fs::write(
            &self.patterns_file,
            serde_json::to_string_pretty(&patterns)?,
        )?;
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TastePatterns {
    pub signal_count: usize,
    pub likes: HashMap<String, usize>,
    pub dislikes: HashMap<String, usize>,
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
