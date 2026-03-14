//! Wright — picks up tasks, does the work, defends its choices.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use ww_workspace::{Task, TaskStatus, TaskStore, TasteStore, Workspace};

use crate::llm::{LlmClient, LlmError};

#[derive(Debug)]
pub struct WorkResult {
    pub task_id: String,
    pub success: bool,
    pub files_changed: Vec<String>,
    pub defense: String,
    pub message: String,
}

pub struct Wright {
    root: PathBuf,
    id: String,
    workspace: Workspace,
    tasks: TaskStore,
    taste: TasteStore,
    llm: LlmClient,
}

impl Wright {
    pub fn new(root: impl AsRef<Path>, llm: LlmClient) -> Self {
        let root = root.as_ref().to_path_buf();
        let meta_dir = root.join(".workwright");
        Self {
            workspace: Workspace::new(&root),
            tasks: TaskStore::new(&meta_dir),
            taste: TasteStore::new(&meta_dir),
            id: "wright-1".to_string(),
            llm,
            root,
        }
    }

    /// Pick up a task and do the work.
    pub async fn work(&self, task_id: &str) -> WorkResult {
        // Claim
        let task = match self.tasks.claim(task_id, &self.id) {
            Ok(t) => t,
            Err(e) => return self.fail(task_id, &format!("claim failed: {e}")),
        };

        let file_scope = scope_to_path(&task.scope);
        let file_content = self.workspace.read_file(&file_scope).unwrap_or_default();
        let taste_guide = self.taste.guide().unwrap_or_default();

        // Read context files
        let mut context = Vec::new();
        for ctx_path in &task.context {
            if let Some(content) = self.workspace.read_file(ctx_path) {
                context.push((ctx_path.clone(), content));
            }
        }

        // Lock
        if let Err(e) = self.workspace.locks.acquire(&file_scope, &self.id, &task.intent) {
            return self.fail(task_id, &format!("lock failed: {e}"));
        }

        // Call LLM for code
        let prompt = build_prompt(&task, &file_content, &taste_guide, &context);
        let new_content = match self.llm.call(&prompt).await {
            Ok(text) => strip_fences(text.trim()),
            Err(e) => {
                self.workspace.locks.release(&file_scope, &self.id).ok();
                return self.fail(task_id, &format!("llm error: {e}"));
            }
        };

        // Call LLM for defense
        let defense_prompt = build_defense_prompt(&task, &new_content);
        let defense = match self.llm.call(&defense_prompt).await {
            Ok(text) => text.trim().to_string(),
            Err(e) => {
                self.workspace.locks.release(&file_scope, &self.id).ok();
                return self.fail(task_id, &format!("defense llm error: {e}"));
            }
        };

        // Write to staging
        if let Err(e) = self.workspace.write_staged(
            &file_scope,
            &new_content,
            &self.id,
            &task.intent,
        ) {
            self.workspace.locks.release(&file_scope, &self.id).ok();
            return self.fail(task_id, &format!("staging write failed: {e}"));
        }

        // Unlock
        self.workspace.locks.release(&file_scope, &self.id).ok();

        // Get change IDs
        let change_ids: Vec<String> = self
            .workspace
            .changelog
            .recent(1)
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.id)
            .collect();

        // Store defense and submit for review
        if let Ok(Some(mut task)) = self.tasks.get(task_id) {
            task.defense = Some(defense.clone());
            self.tasks.update(&task).ok();
        }
        self.tasks.submit(task_id, change_ids).ok();

        info!(task_id, "wright completed work");

        WorkResult {
            task_id: task_id.to_string(),
            success: true,
            files_changed: vec![file_scope],
            defense,
            message: "Completed.".to_string(),
        }
    }

    /// Pick up the next pending task.
    pub async fn work_next(&self) -> Option<WorkResult> {
        let pending = self.tasks.pending().ok()?;
        let task = pending.first()?;
        Some(self.work(&task.id).await)
    }

    fn fail(&self, task_id: &str, reason: &str) -> WorkResult {
        warn!(task_id, reason, "wright failed");

        // Mark task as failed
        if let Ok(Some(mut task)) = self.tasks.get(task_id) {
            task.status = TaskStatus::Failed;
            task.defense = Some(reason.to_string());
            self.tasks.update(&task).ok();
        }

        WorkResult {
            task_id: task_id.to_string(),
            success: false,
            files_changed: vec![],
            defense: reason.to_string(),
            message: format!("Failed: {reason}"),
        }
    }
}

fn scope_to_path(scope: &str) -> String {
    scope.split(':').next().unwrap_or(scope).to_string()
}

fn build_prompt(
    task: &Task,
    file_content: &str,
    taste_guide: &str,
    context: &[(String, String)],
) -> String {
    let mut ctx_block = String::new();
    for (path, content) in context {
        ctx_block.push_str(&format!("### {path}\n```\n{content}\n```\n\n"));
    }
    let ctx_section = if ctx_block.is_empty() {
        String::new()
    } else {
        format!("## Context Files\n{ctx_block}")
    };

    let file_display = if file_content.is_empty() {
        "(new file — create from scratch)"
    } else {
        file_content
    };

    format!(
        r#"You are a wright — a craftsperson who works within a tradition.

## The Two Questions
Every piece of work must answer:
1. **Why are we making this?** {why}
2. **How does it solve it elegantly?** Nothing extra, nothing missing.

## Unix Principles
- Files do one thing
- Functions do one thing
- Readable: code is for humans to understand
- Concise: say what you mean, nothing more
- Max 30 lines per function, max 5 args, max 5 nesting depth

## Taste Guide
{taste_guide}

## Your Task
**Intent:** {intent}
**Why:** {why}
**Scope:** {scope}

{ctx_section}## Target File Content
```
{file_display}
```

## Instructions
Make the change described in the intent. Follow the principles. Return the complete file content — no explanations, no markdown fences, just the code."#,
        why = task.why,
        intent = task.intent,
        scope = task.scope,
        taste_guide = taste_guide,
        ctx_section = ctx_section,
        file_display = file_display,
    )
}

fn build_defense_prompt(task: &Task, code: &str) -> String {
    let truncated = if code.len() > 3000 { &code[..3000] } else { code };
    format!(
        r#"You just completed a piece of work. Now defend it.

**Task:** {intent}
**Why it was needed:** {why}

**What you produced:**
```
{code}
```

Defend your choices. Not what you did — the diff shows that.
Why this form and not another. Why these specific decisions are right.

2-4 sentences. Conceptual, not technical. Go:"#,
        intent = task.intent,
        why = task.why,
        code = truncated,
    )
}

fn strip_fences(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = if lines.first().is_some_and(|l| l.starts_with("```")) {
        1
    } else {
        0
    };
    let end = if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.len() - 1
    } else {
        lines.len()
    };
    lines[start..end].join("\n")
}
