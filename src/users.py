"""
Users — identity and trust in Workwright.

File-based, no database. Same pattern as tasks.jsonl.
"""

import json
import os
import secrets
import time
import uuid
from dataclasses import dataclass, field, asdict
from enum import Enum
from pathlib import Path
from typing import Optional


class UserRole(str, Enum):
    PARTICIPANT = "participant"
    ADMIN = "admin"


@dataclass
class User:
    """A registered participant."""
    id: str                             # uuid hex[:8]
    email: str
    display_name: str
    token: str                          # bearer token for API auth
    trust_score: float = 0.0           # 0.0–1.0
    role: UserRole = UserRole.PARTICIPANT
    created: float = field(default_factory=time.time)

    def is_admin(self) -> bool:
        return self.role == UserRole.ADMIN

    def public_dict(self) -> dict:
        """Safe to expose publicly (no token, no email)."""
        return {
            "id": self.id,
            "display_name": self.display_name,
            "trust_score": self.trust_score,
            "role": self.role.value,
        }

    def profile_dict(self) -> dict:
        """Full profile for /api/me (own user only)."""
        return {
            "id": self.id,
            "email": self.email,
            "display_name": self.display_name,
            "trust_score": self.trust_score,
            "role": self.role.value,
            "created": self.created,
        }


class UserStore:
    """
    Manages registered participants.

    JSONL file-based persistence — same pattern as TaskStore.
    Auto-seeds admin user on first run.
    """

    def __init__(self, workspace_root: str | Path):
        self.root = Path(workspace_root).resolve()
        self.users_file = self.root / ".workwright" / "users.jsonl"
        self.users_file.parent.mkdir(parents=True, exist_ok=True)

        if not self.users_file.exists():
            self.users_file.touch()
            self._seed_admin()

    # ---- Public API ----

    def register(self, email: str, display_name: str) -> User:
        """Register a new participant. Raises ValueError if email already taken."""
        email = email.strip().lower()
        display_name = display_name.strip()
        if not email or not display_name:
            raise ValueError("email and display_name are required")
        if self.get_by_email(email):
            raise ValueError(f"Email {email!r} already registered")

        user = User(
            id=uuid.uuid4().hex[:8],
            email=email,
            display_name=display_name,
            token=secrets.token_urlsafe(24),
        )
        self._append(user)
        return user

    def get_by_token(self, token: str) -> Optional["User"]:
        """Resolve a bearer token to a User."""
        if not token:
            return None
        for user in self.all():
            if user.token == token:
                return user
        return None

    def get_by_id(self, user_id: str) -> Optional["User"]:
        for user in self.all():
            if user.id == user_id:
                return user
        return None

    def get_by_email(self, email: str) -> Optional["User"]:
        email = email.strip().lower()
        for user in self.all():
            if user.email == email:
                return user
        return None

    def update_trust(self, user_id: str, delta: float) -> "User":
        """
        Adjust a user's trust score by delta, clamped to [0.0, 1.0].
        Returns updated user.
        """
        user = self.get_by_id(user_id)
        if not user:
            raise ValueError(f"User {user_id} not found")
        user.trust_score = max(0.0, min(1.0, user.trust_score + delta))
        self._update(user)
        return user

    def all(self) -> list["User"]:
        """All users."""
        users = []
        for line in self.users_file.read_text().strip().split("\n"):
            if not line.strip():
                continue
            data = json.loads(line)
            data["role"] = UserRole(data["role"])
            users.append(User(**data))
        return users

    # ---- Internal ----

    def _seed_admin(self):
        """Create Billy's admin user on first run."""
        admin_token = os.environ.get("WW_SUBMIT_TOKEN", secrets.token_urlsafe(24))
        admin = User(
            id=uuid.uuid4().hex[:8],
            email="billy@billy.xxx",
            display_name="billy",
            token=admin_token,
            trust_score=1.0,
            role=UserRole.ADMIN,
        )
        self._append(admin)

    def _append(self, user: User):
        with open(self.users_file, "a") as f:
            d = asdict(user)
            d["role"] = user.role.value
            f.write(json.dumps(d) + "\n")

    def _update(self, user: User):
        lines = self.users_file.read_text().strip().split("\n")
        updated = []
        for line in lines:
            if not line.strip():
                continue
            data = json.loads(line)
            if data["id"] == user.id:
                d = asdict(user)
                d["role"] = user.role.value
                updated.append(json.dumps(d))
            else:
                updated.append(line)
        self.users_file.write_text("\n".join(updated) + "\n")
