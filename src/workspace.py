"""
Workspace — manages the shared codebase.

Enforces the Unix principle: files do one thing, functions do one thing.
Tracks what's being worked on (locks) and what's changed (changelog).
"""

import json
import hashlib
import time
from pathlib import Path
from dataclasses import dataclass, field, asdict
from typing import Optional


@dataclass
class Lock:
    """A fine-grained lock on a unit of work."""
    path: str           # file path relative to workspace root
    agent_id: str       # who holds the lock
    intent: str         # what they're doing (human-readable)
    acquired: float     # timestamp
    expires: float      # auto-release after this time


@dataclass
class Change:
    """A single atomic change to the workspace."""
    id: str                     # unique hash
    path: str                   # file changed
    intent: str                 # why this change was made
    agent_id: str               # who made it
    timestamp: float
    before_hash: Optional[str]  # hash of file before change
    after_hash: Optional[str]   # hash of file after change
    taste_score: Optional[float] = None  # human evaluation (-1 to 1)
    taste_note: Optional[str] = None     # human's reason


class Workspace:
    """
    A shared workspace for agents.

    Not git. No branches, no history tree. Just:
    - The current state of files
    - Locks on what's being worked on
    - A changelog of what changed and why
    """

    def __init__(self, root: str | Path):
        self.root = Path(root).resolve()
        self.meta_dir = self.root / ".workwright"
        self.meta_dir.mkdir(parents=True, exist_ok=True)
        self._locks_file = self.meta_dir / "locks.json"
        self._changelog_file = self.meta_dir / "changelog.jsonl"
        self._init_files()

    def _init_files(self):
        if not self._locks_file.exists():
            self._locks_file.write_text("{}")
        if not self._changelog_file.exists():
            self._changelog_file.touch()

    # --- Locking ---

    def lock(self, path: str, agent_id: str, intent: str, ttl: int = 300) -> Lock:
        """Acquire a lock on a file. Raises if already locked."""
        locks = self._read_locks()
        self._expire_stale(locks)

        if path in locks:
            holder = locks[path]
            raise RuntimeError(
                f"{path} locked by {holder['agent_id']}: {holder['intent']}"
            )

        lk = Lock(
            path=path,
            agent_id=agent_id,
            intent=intent,
            acquired=time.time(),
            expires=time.time() + ttl,
        )
        locks[path] = asdict(lk)
        self._write_locks(locks)
        return lk

    def unlock(self, path: str, agent_id: str):
        """Release a lock. Only the holder can release."""
        locks = self._read_locks()
        if path in locks and locks[path]["agent_id"] == agent_id:
            del locks[path]
            self._write_locks(locks)

    def locked_by(self, path: str) -> Optional[dict]:
        """Who holds the lock on this path?"""
        locks = self._read_locks()
        self._expire_stale(locks)
        return locks.get(path)

    def list_locks(self) -> dict:
        """All current locks."""
        locks = self._read_locks()
        self._expire_stale(locks)
        return locks

    # --- Changes ---

    def record_change(self, path: str, agent_id: str, intent: str,
                      before: Optional[str] = None, after: Optional[str] = None) -> Change:
        """Record an atomic change to the changelog."""
        change = Change(
            id=hashlib.sha256(f"{path}:{time.time()}:{agent_id}".encode()).hexdigest()[:12],
            path=path,
            intent=intent,
            agent_id=agent_id,
            timestamp=time.time(),
            before_hash=self._hash_content(before) if before else None,
            after_hash=self._hash_content(after) if after else None,
        )
        with open(self._changelog_file, "a") as f:
            f.write(json.dumps(asdict(change)) + "\n")
        return change

    def recent_changes(self, limit: int = 20) -> list[Change]:
        """Last N changes."""
        lines = self._changelog_file.read_text().strip().split("\n")
        lines = [l for l in lines if l]
        entries = [json.loads(l) for l in lines[-limit:]]
        return [Change(**e) for e in reversed(entries)]

    def get_change(self, change_id: str) -> Optional[Change]:
        """Fetch a specific change by ID."""
        for line in self._changelog_file.read_text().strip().split("\n"):
            if not line:
                continue
            entry = json.loads(line)
            if entry["id"] == change_id:
                return Change(**entry)
        return None

    def update_change_taste(self, change_id: str, score: float, note: str = ""):
        """Apply a taste signal to a recorded change."""
        lines = self._changelog_file.read_text().strip().split("\n")
        updated = []
        for line in lines:
            if not line:
                continue
            entry = json.loads(line)
            if entry["id"] == change_id:
                entry["taste_score"] = max(-1.0, min(1.0, score))
                entry["taste_note"] = note
            updated.append(json.dumps(entry))
        self._changelog_file.write_text("\n".join(updated) + "\n")

    # --- File operations ---

    def read_file(self, path: str) -> Optional[str]:
        """Read a file from the workspace."""
        fp = self.root / path
        if fp.exists() and fp.is_file():
            return fp.read_text()
        return None

    def write_file(self, path: str, content: str, agent_id: str, intent: str):
        """Write a file, record the change. Must hold the lock."""
        lock = self.locked_by(path)
        if lock and lock["agent_id"] != agent_id:
            raise RuntimeError(f"{path} locked by {lock['agent_id']}")

        fp = self.root / path
        before = fp.read_text() if fp.exists() else None
        fp.parent.mkdir(parents=True, exist_ok=True)
        fp.write_text(content)
        self.record_change(path, agent_id, intent, before, content)

    # --- Internals ---

    def _read_locks(self) -> dict:
        return json.loads(self._locks_file.read_text())

    def _write_locks(self, locks: dict):
        self._locks_file.write_text(json.dumps(locks, indent=2))

    def _expire_stale(self, locks: dict):
        now = time.time()
        expired = [k for k, v in locks.items() if v["expires"] < now]
        for k in expired:
            del locks[k]
        if expired:
            self._write_locks(locks)

    @staticmethod
    def _hash_content(content: str) -> str:
        return hashlib.sha256(content.encode()).hexdigest()[:16]
