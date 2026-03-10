"""
Task — atomic units of work.

A task is an intent declaration: what needs to happen, why, and at what grain.
Tasks are small enough for a human to evaluate the result in seconds.
"""

import json
import time
import uuid
from pathlib import Path
from dataclasses import dataclass, field, asdict
from enum import Enum
from typing import Optional


class TaskStatus(str, Enum):
    PENDING = "pending"         # waiting to be picked up
    ACTIVE = "active"           # an agent is working on it
    REVIEW = "review"           # done, waiting for taste evaluation
    ACCEPTED = "accepted"       # human approved
    REJECTED = "rejected"       # human rejected (with reason)
    ABANDONED = "abandoned"     # dropped


@dataclass
class Task:
    """A single evaluable unit of work."""
    id: str
    intent: str                         # what: "add input validation to parse_date()"
    why: str                            # why: "user dates crash the pipeline"
    scope: str                          # file or function this touches
    status: TaskStatus = TaskStatus.PENDING
    agent_id: Optional[str] = None      # who's working on it
    created: float = field(default_factory=time.time)
    started: Optional[float] = None
    completed: Optional[float] = None
    change_ids: list[str] = field(default_factory=list)  # workspace changelog refs
    taste_score: Optional[float] = None
    taste_note: Optional[str] = None


class TaskStore:
    """
    Manages the task queue.

    Tasks in, evaluated results out. Simple file-based persistence.
    """

    def __init__(self, workspace_root: str | Path):
        self.root = Path(workspace_root).resolve()
        self.tasks_file = self.root / ".workwright" / "tasks.jsonl"
        self.tasks_file.parent.mkdir(parents=True, exist_ok=True)
        if not self.tasks_file.exists():
            self.tasks_file.touch()

    def create(self, intent: str, why: str, scope: str) -> Task:
        """Create a new task."""
        task = Task(
            id=uuid.uuid4().hex[:8],
            intent=intent,
            why=why,
            scope=scope,
        )
        self._append(task)
        return task

    def decompose(self, goal: str, why: str, scopes: list[str]) -> list[Task]:
        """
        Break a high-level goal into atomic tasks, one per scope.

        This is the simplest decomposition: one task per file/function.
        A smarter version would use an LLM to decompose intelligently.
        """
        tasks = []
        for scope in scopes:
            task = self.create(
                intent=f"{goal} — {scope}",
                why=why,
                scope=scope,
            )
            tasks.append(task)
        return tasks

    def claim(self, task_id: str, agent_id: str) -> Task:
        """Agent claims a task to work on."""
        task = self.get(task_id)
        if not task:
            raise ValueError(f"Task {task_id} not found")
        if task.status != TaskStatus.PENDING:
            raise RuntimeError(f"Task {task_id} is {task.status}, not claimable")
        task.status = TaskStatus.ACTIVE
        task.agent_id = agent_id
        task.started = time.time()
        self._update(task)
        return task

    def submit(self, task_id: str, change_ids: list[str]) -> Task:
        """Agent submits work for review."""
        task = self.get(task_id)
        if not task:
            raise ValueError(f"Task {task_id} not found")
        task.status = TaskStatus.REVIEW
        task.completed = time.time()
        task.change_ids = change_ids
        self._update(task)
        return task

    def evaluate(self, task_id: str, score: float, note: str = "") -> Task:
        """Human evaluates a completed task. Score: -1 (reject) to 1 (accept)."""
        task = self.get(task_id)
        if not task:
            raise ValueError(f"Task {task_id} not found")
        task.taste_score = max(-1.0, min(1.0, score))
        task.taste_note = note
        task.status = TaskStatus.ACCEPTED if score > 0 else TaskStatus.REJECTED
        self._update(task)
        return task

    def pending(self) -> list[Task]:
        """Tasks waiting for an agent."""
        return [t for t in self.all() if t.status == TaskStatus.PENDING]

    def for_review(self) -> list[Task]:
        """Tasks waiting for human evaluation."""
        return [t for t in self.all() if t.status == TaskStatus.REVIEW]

    def get(self, task_id: str) -> Optional[Task]:
        """Fetch a task by ID."""
        for task in self.all():
            if task.id == task_id:
                return task
        return None

    def all(self) -> list[Task]:
        """All tasks."""
        tasks = []
        for line in self.tasks_file.read_text().strip().split("\n"):
            if not line:
                continue
            data = json.loads(line)
            data["status"] = TaskStatus(data["status"])
            tasks.append(Task(**data))
        return tasks

    def _append(self, task: Task):
        with open(self.tasks_file, "a") as f:
            f.write(json.dumps(asdict(task)) + "\n")

    def _update(self, task: Task):
        lines = self.tasks_file.read_text().strip().split("\n")
        updated = []
        for line in lines:
            if not line:
                continue
            data = json.loads(line)
            if data["id"] == task.id:
                updated.append(json.dumps(asdict(task)))
            else:
                updated.append(line)
        self.tasks_file.write_text("\n".join(updated) + "\n")
