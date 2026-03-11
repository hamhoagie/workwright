"""
Taste — the learned sensibility.

Accumulates human signals (accept/reject + why) and builds a model
of what "good" means for this human on this project.

Not a style guide. Not rules. A gradient.
"""

import json
import time
from pathlib import Path
from dataclasses import dataclass, asdict, field
from typing import Optional
from collections import Counter


@dataclass
class Signal:
    """A single taste signal from a human."""
    timestamp: float
    change_id: Optional[str]
    task_id: Optional[str]
    score: float                # -1.0 to 1.0
    reason: str
    tags: list[str] = field(default_factory=list)


# --- Tag extraction keywords ---

_POSITIVE_TAGS = {
    "clean": "clean-code", "simple": "simplicity", "readable": "readability",
    "elegant": "elegance", "concise": "conciseness", "clear": "clarity",
    "obvious": "obviousness", "small": "small-units", "focused": "focus",
    "unix": "unix-philosophy", "one thing": "single-responsibility",
}

_NEGATIVE_TAGS = {
    "complex": "complexity", "bloat": "bloat", "clever": "too-clever",
    "confusing": "confusion", "over-engineer": "over-engineering",
    "unnecessary": "unnecessary", "too many": "excess",
    "verbose": "verbosity", "magic": "magic", "framework": "framework-brain",
}


def extract_tags(reason: str) -> list[str]:
    """Extract taste tags from a human's reason."""
    tags = []
    lower = reason.lower()
    for keyword, tag in _POSITIVE_TAGS.items():
        if keyword in lower:
            tags.append(tag)
    for keyword, tag in _NEGATIVE_TAGS.items():
        if keyword in lower:
            tags.append(tag)
    return tags or ["general"]


class TasteStore:
    """
    Stores and learns from human taste signals.
    Starts empty, accumulates over time.
    """

    def __init__(self, workspace_root: str | Path):
        self.root = Path(workspace_root).resolve()
        self._dir = self.root / ".workwright"
        self._dir.mkdir(parents=True, exist_ok=True)
        self.signals_file = self._dir / "taste.jsonl"
        self.patterns_file = self._dir / "taste_patterns.json"
        if not self.signals_file.exists():
            self.signals_file.touch()

    def record(self, score: float, reason: str,
               change_id: str = None, task_id: str = None,
               tags: list[str] = None) -> Signal:
        """Record a human taste signal."""
        signal = Signal(
            timestamp=time.time(),
            change_id=change_id,
            task_id=task_id,
            score=max(-1.0, min(1.0, score)),
            reason=reason,
            tags=tags or extract_tags(reason),
        )
        with open(self.signals_file, "a") as f:
            f.write(json.dumps(asdict(signal)) + "\n")
        self._update_patterns()
        return signal

    def patterns(self) -> dict:
        """Current taste patterns."""
        if self.patterns_file.exists():
            return json.loads(self.patterns_file.read_text())
        return {"likes": {}, "dislikes": {}, "principles": [], "signal_count": 0}

    def guide(self) -> str:
        """Generate natural language taste guide for wrights."""
        p = self.patterns()
        if p["signal_count"] < 2:
            return "Not enough taste data yet. Follow Unix principles: single responsibility, readable, concise."
        parts = ["## Taste Guide (learned from human feedback)\n"]
        parts.append(_format_tags("This human values:", p.get("likes", {})))
        parts.append(_format_tags("This human rejects:", p.get("dislikes", {})))
        parts.append(_format_principles(p.get("principles", [])))
        parts.append(f"*Based on {p['signal_count']} taste signals.*")
        return "\n".join(p for p in parts if p)

    def all_signals(self) -> list[Signal]:
        """All recorded signals."""
        signals = []
        for line in self.signals_file.read_text().strip().split("\n"):
            if line:
                signals.append(Signal(**json.loads(line)))
        return signals

    def _update_patterns(self):
        """Recompute taste patterns from all signals."""
        signals = self.all_signals()
        likes, dislikes, reasons = Counter(), Counter(), []

        for s in signals:
            target = likes if s.score > 0 else dislikes if s.score < 0 else None
            if target is not None:
                for tag in s.tags:
                    target[tag] += 1
            if s.score > 0 and s.reason:
                reasons.append(s.reason)

        principles = _dedupe_reasons(reasons[-20:])
        patterns = {
            "likes": dict(likes.most_common(20)),
            "dislikes": dict(dislikes.most_common(20)),
            "principles": principles[-10:],
            "signal_count": len(signals),
        }
        self.patterns_file.write_text(json.dumps(patterns, indent=2))


def _format_tags(header: str, tags: dict) -> str:
    """Format a tag section for the taste guide."""
    if not tags:
        return ""
    lines = [f"**{header}**"]
    for tag, count in sorted(tags.items(), key=lambda x: -x[1])[:10]:
        lines.append(f"- {tag} (seen {count}×)")
    return "\n".join(lines) + "\n"


def _format_principles(principles: list[str]) -> str:
    """Format principles for the taste guide."""
    if not principles:
        return ""
    lines = ["**Extracted principles:**"]
    for p in principles:
        lines.append(f"- {p}")
    return "\n".join(lines) + "\n"


def _dedupe_reasons(reasons: list[str]) -> list[str]:
    """Deduplicate reasons by prefix."""
    seen, result = set(), []
    for r in reasons:
        key = r.lower().strip()[:50]
        if key not in seen:
            seen.add(key)
            result.append(r)
    return result
