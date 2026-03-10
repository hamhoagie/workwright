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
    change_id: Optional[str]    # links to workspace changelog
    task_id: Optional[str]      # links to task
    score: float                # -1.0 (reject) to 1.0 (accept)
    reason: str                 # why — this is the learning data
    tags: list[str] = field(default_factory=list)  # extracted patterns


class TasteStore:
    """
    Stores and learns from human taste signals.

    The taste model starts empty and accumulates. Over time it builds
    a picture of what this human values, rejects, and why.
    """

    def __init__(self, workspace_root: str | Path):
        self.root = Path(workspace_root).resolve()
        self.signals_file = self.root / ".agenthub" / "taste.jsonl"
        self.patterns_file = self.root / ".agenthub" / "taste_patterns.json"
        self.signals_file.parent.mkdir(parents=True, exist_ok=True)
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
            tags=tags or self._extract_tags(reason),
        )
        with open(self.signals_file, "a") as f:
            f.write(json.dumps(asdict(signal)) + "\n")

        # Update patterns after each signal
        self._update_patterns()
        return signal

    def patterns(self) -> dict:
        """
        Current taste patterns — what does this human value?

        Returns a dict of:
        - likes: tags/patterns that correlate with positive scores
        - dislikes: tags/patterns that correlate with negative scores
        - principles: extracted general preferences
        - signal_count: how many signals we've learned from
        """
        if self.patterns_file.exists():
            return json.loads(self.patterns_file.read_text())
        return {"likes": {}, "dislikes": {}, "principles": [], "signal_count": 0}

    def guide(self) -> str:
        """
        Generate a natural language taste guide for agents.

        This is what gets injected into agent prompts so they
        understand what this human values.
        """
        p = self.patterns()
        if p["signal_count"] < 3:
            return "Not enough taste data yet. Follow Unix principles: single responsibility, readable, concise."

        lines = ["## Taste Guide (learned from human feedback)\n"]

        if p["likes"]:
            lines.append("**This human values:**")
            for tag, count in sorted(p["likes"].items(), key=lambda x: -x[1])[:10]:
                lines.append(f"- {tag} (seen {count}×)")
            lines.append("")

        if p["dislikes"]:
            lines.append("**This human rejects:**")
            for tag, count in sorted(p["dislikes"].items(), key=lambda x: -x[1])[:10]:
                lines.append(f"- {tag} (seen {count}×)")
            lines.append("")

        if p["principles"]:
            lines.append("**Extracted principles:**")
            for principle in p["principles"]:
                lines.append(f"- {principle}")
            lines.append("")

        lines.append(f"*Based on {p['signal_count']} taste signals.*")
        return "\n".join(lines)

    def all_signals(self) -> list[Signal]:
        """All recorded signals."""
        signals = []
        for line in self.signals_file.read_text().strip().split("\n"):
            if not line:
                continue
            data = json.loads(line)
            signals.append(Signal(**data))
        return signals

    def _extract_tags(self, reason: str) -> list[str]:
        """
        Extract taste tags from a human's reason.

        Simple keyword extraction. A smarter version would use
        an LLM to extract semantic patterns.
        """
        tags = []
        reason_lower = reason.lower()

        # Positive patterns
        positive_keywords = {
            "clean": "clean-code",
            "simple": "simplicity",
            "readable": "readability",
            "elegant": "elegance",
            "concise": "conciseness",
            "clear": "clarity",
            "obvious": "obviousness",
            "small": "small-units",
            "focused": "focus",
            "unix": "unix-philosophy",
            "one thing": "single-responsibility",
        }

        # Negative patterns
        negative_keywords = {
            "complex": "complexity",
            "bloat": "bloat",
            "clever": "too-clever",
            "confusing": "confusion",
            "over-engineer": "over-engineering",
            "unnecessary": "unnecessary",
            "too many": "excess",
            "verbose": "verbosity",
            "magic": "magic",
            "framework": "framework-brain",
        }

        for keyword, tag in positive_keywords.items():
            if keyword in reason_lower:
                tags.append(tag)

        for keyword, tag in negative_keywords.items():
            if keyword in reason_lower:
                tags.append(tag)

        return tags if tags else ["general"]

    def _update_patterns(self):
        """Recompute taste patterns from all signals."""
        signals = self.all_signals()

        likes = Counter()
        dislikes = Counter()
        reasons_positive = []
        reasons_negative = []

        for s in signals:
            if s.score > 0:
                for tag in s.tags:
                    likes[tag] += 1
                if s.reason:
                    reasons_positive.append(s.reason)
            elif s.score < 0:
                for tag in s.tags:
                    dislikes[tag] += 1
                if s.reason:
                    reasons_negative.append(s.reason)

        # Extract principles from reasons (simple dedup for now)
        principles = []
        seen = set()
        for r in reasons_positive[-20:]:  # last 20 positive reasons
            key = r.lower().strip()[:50]
            if key not in seen:
                seen.add(key)
                principles.append(r)

        patterns = {
            "likes": dict(likes.most_common(20)),
            "dislikes": dict(dislikes.most_common(20)),
            "principles": principles[-10:],  # keep last 10
            "signal_count": len(signals),
        }
        self.patterns_file.write_text(json.dumps(patterns, indent=2))
