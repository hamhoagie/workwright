"""
Wright — the one who does the work.

A wright picks up a task, reads the taste guide, does the work,
and submits it for human evaluation. It works within the container
the human defined.

Not an "agent." A wright. One who works within a craft tradition.
"""

import json
import os
import time
from pathlib import Path
from dataclasses import dataclass
from typing import Optional

from .workspace import Workspace
from .task import TaskStore, Task, TaskStatus
from .taste import TasteStore
from .evaluate import evaluate_file


@dataclass
class WorkResult:
    """What a wright produced."""
    task_id: str
    success: bool
    files_changed: list[str]
    change_ids: list[str]
    evaluation_score: float     # self-eval against principles
    message: str                # what the wright did


def _call_llm(prompt: str, model: str = "sonnet") -> str:
    """
    Call an LLM. Thin wrapper — keeps the wright model-agnostic.

    Uses OpenClaw's sessions_spawn pattern via CLI for now.
    Can be swapped to direct API calls later.
    """
    # For MVP: use Anthropic API directly via environment
    import anthropic

    client = anthropic.Anthropic()
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=4096,
        messages=[{"role": "user", "content": prompt}],
    )
    return response.content[0].text


def build_prompt(task: Task, file_content: str, taste_guide: str,
                  context: dict[str, str] = None) -> str:
    """Construct the wright's working prompt."""
    context_block = ""
    if context:
        parts = []
        for path, content in context.items():
            parts.append(f"### {path}\n```\n{content}\n```")
        context_block = "## Context Files\n" + "\n\n".join(parts) + "\n"

    return f"""You are a wright — a craftsperson who works within a tradition.

## The Two Questions
Every piece of work must answer:
1. **Why are we making this?** {task.why}
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
**Intent:** {task.intent}
**Why:** {task.why}
**Scope:** {task.scope}

{context_block}## Target File Content
```
{file_content or "(new file — create from scratch)"}
```

## Instructions
1. Make the change described in the intent
2. Follow the Unix principles strictly
3. Follow the taste guide
4. Return ONLY the complete updated file content
5. No explanations, no markdown fences, just the code

Return the complete file content:"""


class Wright:
    """
    A wright that can pick up tasks and do work.

    The wright reads the taste guide, understands the intent,
    does the work, self-evaluates, and submits for review.
    """

    def __init__(self, workspace_root: str | Path, wright_id: str = "wright-1"):
        self.root = Path(workspace_root).resolve()
        self.wright_id = wright_id
        self.workspace = Workspace(workspace_root)
        self.tasks = TaskStore(workspace_root)
        self.taste = TasteStore(workspace_root)

    def work(self, task_id: str) -> WorkResult:
        """Pick up a task and do the work."""
        # Claim the task
        task = self.tasks.claim(task_id, self.wright_id)

        # Read context
        file_content = self.workspace.read_file(task.scope)
        taste_guide = self.taste.guide()
        context = {}
        for ctx_path in (task.context or []):
            ctx_content = self.workspace.read_file(ctx_path)
            if ctx_content:
                context[ctx_path] = ctx_content

        # Lock the file
        try:
            self.workspace.lock(task.scope, self.wright_id, task.intent)
        except RuntimeError:
            return WorkResult(
                task_id=task_id, success=False, files_changed=[],
                change_ids=[], evaluation_score=0.0,
                message=f"Could not lock {task.scope}",
            )

        # Build prompt and call LLM
        prompt = build_prompt(task, file_content, taste_guide, context)
        new_content = _call_llm(prompt)

        # Clean up LLM response (strip markdown fences if present)
        new_content = _clean_response(new_content)

        # Write the file
        self.workspace.write_file(
            task.scope, new_content, self.wright_id, task.intent
        )

        # Self-evaluate
        eval_result = evaluate_file(self.root / task.scope)

        # Unlock
        self.workspace.unlock(task.scope, self.wright_id)

        # Get change IDs
        changes = self.workspace.recent_changes(limit=1)
        change_ids = [c.id for c in changes]

        # Submit for review
        self.tasks.submit(task_id, change_ids)

        return WorkResult(
            task_id=task_id,
            success=True,
            files_changed=[task.scope],
            change_ids=change_ids,
            evaluation_score=eval_result.score,
            message=f"Completed. Self-eval: {eval_result.score:.2f}",
        )

    def work_next(self) -> Optional[WorkResult]:
        """Pick up the next pending task and work on it."""
        pending = self.tasks.pending()
        if not pending:
            return None
        return self.work(pending[0].id)


def _clean_response(text: str) -> str:
    """Strip markdown code fences from LLM response."""
    lines = text.strip().split("\n")
    if lines and lines[0].startswith("```"):
        lines = lines[1:]
    if lines and lines[-1].strip() == "```":
        lines = lines[:-1]
    return "\n".join(lines)
