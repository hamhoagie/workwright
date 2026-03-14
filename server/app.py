"""
Workwright API — serves the live workspace.

Four endpoints. Nothing extra. Now with multi-user identity.
"""

import json
import os
import time
import hashlib
import hmac
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler
from threading import Thread

# Auth token for write operations (brief + crit)
# Still used as the admin's token for backward compatibility (via seed)
SUBMIT_TOKEN = os.environ.get("WW_SUBMIT_TOKEN", "")

# Workspace root — the workwright project itself
ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"

# Import workwright internals
import sys
sys.path.insert(0, str(ROOT))

from src.workspace import Workspace
from src.task import TaskStore, TaskStatus
from src.taste import TasteStore
from src.wright import Wright
from src.users import UserStore, User


store_tasks = TaskStore(ROOT)
store_taste = TasteStore(ROOT)
store_users = UserStore(ROOT)


def _task_json(t):
    """Task to JSON-safe dict."""
    return {
        "id": t.id,
        "intent": t.intent,
        "why": t.why,
        "scope": t.scope,
        "status": t.status.value,
        "created": t.created,
        "agent_id": t.agent_id,
        "defense": t.defense,
        "change_ids": t.change_ids,
        "taste_score": t.taste_score,
        "taste_note": t.taste_note,
        "submitted_by": t.submitted_by,
        "submitted_by_name": t.submitted_by_name,
        "critted_by": t.critted_by,
        "critted_by_name": t.critted_by_name,
    }


def _read_body(handler):
    """Read and parse JSON body."""
    length = int(handler.headers.get("Content-Length", 0))
    raw = handler.rfile.read(length)
    return json.loads(raw) if raw else {}


def _ip(handler):
    """Client IP, respecting X-Forwarded-For."""
    addr = handler.client_address[0]
    forwarded = handler.headers.get("X-Forwarded-For")
    if forwarded:
        addr = forwarded.split(",")[0].strip()
    return addr


# ---- Rate limiters ----
# Keyed by (kind, key) -> (count, window_start)
_rate_counts = {}

RATE_WINDOW = 3600  # 1 hour

_RATE_LIMITS = {
    "submit_ip": 10,      # task submissions per IP per hour
    "wright_user": 5,     # wright runs per non-admin user per hour
    "register_ip": 3,     # registrations per IP per hour
}


def _check_rate(kind: str, key: str, limit: int = None) -> bool:
    """Return True if allowed. Uses _RATE_LIMITS[kind] unless limit is given."""
    if limit is None:
        limit = _RATE_LIMITS.get(kind, 10)
    now = time.time()
    bucket = (kind, key)
    count, start = _rate_counts.get(bucket, (0, now))
    if now - start > RATE_WINDOW:
        _rate_counts[bucket] = (1, now)
        return True
    if count >= limit:
        return False
    _rate_counts[bucket] = (count + 1, start)
    return True


class Handler(BaseHTTPRequestHandler):
    """Handles API and static file requests."""

    def log_message(self, fmt, *args):
        """Quiet logs."""
        pass

    def _json(self, data, status=200):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def _cors(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, Authorization")
        self.end_headers()

    def _get_token(self) -> str:
        """Extract bearer token from Authorization header."""
        auth = self.headers.get("Authorization", "")
        if auth.startswith("Bearer "):
            return auth[7:].strip()
        return ""

    def _check_auth(self) -> bool:
        """
        Verify bearer token. Returns True if authed.
        Attaches resolved User to self._user (None if unauthenticated).
        """
        self._user = None
        token = self._get_token()

        if not token:
            # No token at all — only allowed if no SUBMIT_TOKEN configured
            return not SUBMIT_TOKEN

        # Resolve token to user
        user = store_users.get_by_token(token)
        if user:
            self._user = user
            return True

        # Backward compat: raw SUBMIT_TOKEN still works (maps to admin)
        if SUBMIT_TOKEN and hmac.compare_digest(token, SUBMIT_TOKEN):
            # Find the admin user (seeded with this token)
            user = store_users.get_by_token(token)
            if user:
                self._user = user
            return True

        return False

    def do_OPTIONS(self):
        self._cors()

    def do_GET(self):
        if self.path == "/api/tasks":
            return self._get_tasks()
        if self.path == "/api/taste":
            return self._get_taste()
        if self.path == "/api/users":
            return self._get_users()
        if self.path == "/api/me":
            return self._get_me()
        if self.path.startswith("/api/preview/"):
            return self._get_preview(self.path.split("/")[-1])
        if self.path.startswith("/api/changes/"):
            return self._get_change(self.path.split("/")[-1])
        return self._serve_static()

    def do_POST(self):
        if self.path == "/api/tasks":
            return self._post_task()
        if self.path == "/api/crit":
            return self._post_crit()
        if self.path == "/api/register":
            return self._post_register()
        self._json({"error": "not found"}, 404)

    def _get_tasks(self):
        tasks = store_tasks.all()
        tasks.sort(key=lambda t: t.created, reverse=True)
        self._json([_task_json(t) for t in tasks[:50]])

    def _get_taste(self):
        guide = store_taste.guide()
        patterns = store_taste.patterns()
        self._json({
            "text": guide,
            "signal_count": patterns.get("signal_count", 0),
            "likes": patterns.get("likes", {}),
            "dislikes": patterns.get("dislikes", {}),
        })

    def _get_users(self):
        """Public list of participants (no tokens, no emails)."""
        users = store_users.all()
        self._json([u.public_dict() for u in users])

    def _get_me(self):
        """Own profile — requires auth."""
        if not self._check_auth() or not self._user:
            return self._json({"error": "unauthorized"}, 401)
        self._json(self._user.profile_dict())

    def _get_preview(self, change_id):
        """Serve the staged file as a rendered HTML page for review."""
        ws = Workspace(ROOT)
        changes = ws.recent_changes(limit=100)
        for c in changes:
            if c.id == change_id:
                # Try staged first, fall back to live
                staged = ws.read_staged(c.path)
                if staged and c.path.endswith(".html"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html")
                    self.send_header("Access-Control-Allow-Origin", "*")
                    self.end_headers()
                    self.wfile.write(staged.encode())
                    return
                filepath = ROOT / c.path
                if filepath.exists() and c.path.endswith(".html"):
                    self.send_response(200)
                    self.send_header("Content-Type", "text/html")
                    self.send_header("Access-Control-Allow-Origin", "*")
                    self.end_headers()
                    self.wfile.write(filepath.read_bytes())
                    return
        self._json({"error": "preview not available"}, 404)

    def _get_change(self, change_id):
        ws = Workspace(ROOT)
        changes = ws.recent_changes(limit=100)
        for c in changes:
            if c.id == change_id:
                content = None
                filepath = ROOT / c.path
                if filepath.exists():
                    try:
                        content = filepath.read_text()
                    except Exception:
                        content = "(binary file)"
                return self._json({
                    "id": c.id,
                    "path": c.path,
                    "intent": c.intent,
                    "agent_id": c.agent_id,
                    "content": content,
                })
        self._json({"error": "change not found"}, 404)

    def _post_register(self):
        """Register a new participant. Rate limited per IP."""
        ip = _ip(self)
        if not _check_rate("register_ip", ip):
            return self._json({"error": "rate limited"}, 429)

        body = _read_body(self)
        email = body.get("email", "").strip()
        display_name = body.get("display_name", "").strip()

        if not email or not display_name:
            return self._json({"error": "email and display_name required"}, 400)
        if len(email) > 200 or len(display_name) > 60:
            return self._json({"error": "too long"}, 400)

        try:
            user = store_users.register(email, display_name)
        except ValueError as e:
            return self._json({"error": str(e)}, 409)

        # Record registration in the feed
        store_tasks.create(
            intent=f"{display_name} joined.",
            why="New participant registered.",
            scope="system:registration",
            context=[],
            submitted_by=user.id,
            submitted_by_name=user.display_name,
        )

        self._json({
            "user_id": user.id,
            "token": user.token,
            "display_name": user.display_name,
            "trust_score": user.trust_score,
        }, 201)

    def _post_task(self):
        if not self._check_auth():
            return self._json({"error": "unauthorized"}, 401)
        ip = _ip(self)
        if not _check_rate("submit_ip", ip):
            return self._json({"error": "rate limited"}, 429)

        # Rate limit wright runs for non-admin users
        if self._user and not self._user.is_admin():
            if not _check_rate("wright_user", self._user.id):
                return self._json({"error": "wright rate limit: 5 per hour"}, 429)

        body = _read_body(self)
        intent = body.get("intent", "").strip()
        why = body.get("why", "").strip()

        if not intent or not why:
            return self._json({"error": "intent and why required"}, 400)
        if len(intent) > 200 or len(why) > 500:
            return self._json({"error": "too long"}, 400)

        scope = body.get("scope", "site/index.html")
        file_path = scope.split(":")[0] if ":" in scope else scope
        if not file_path:
            file_path = "site/index.html"

        task = store_tasks.create(
            intent=intent,
            why=why,
            scope=scope,
            context=[file_path],
        )

        # Tag with submitter identity
        if self._user:
            task.submitted_by = self._user.id
            task.submitted_by_name = self._user.display_name
            store_tasks._update(task)

        _run_wright_async(task.id)

        self._json(_task_json(task), 201)

    def _post_crit(self):
        if not self._check_auth():
            return self._json({"error": "unauthorized"}, 401)

        # Only admins can crit (for now)
        if self._user and not self._user.is_admin():
            return self._json({"error": "only admins can crit"}, 403)

        ip = _ip(self)
        if not _check_rate("submit_ip", ip):
            return self._json({"error": "rate limited"}, 429)

        body = _read_body(self)
        task_id = body.get("task_id", "").strip()
        score = body.get("score", 0)
        reason = body.get("reason", "").strip()

        if not task_id:
            return self._json({"error": "task_id required"}, 400)

        task = store_tasks.get(task_id)
        if not task:
            return self._json({"error": "task not found"}, 404)

        score = max(-1.0, min(1.0, float(score)))

        # Tag critter
        if self._user:
            task.critted_by = self._user.id
            task.critted_by_name = self._user.display_name

        store_tasks.evaluate(task_id, score, reason)
        # Re-fetch to get updated state, then re-apply critter tag
        task = store_tasks.get(task_id)
        if self._user:
            task.critted_by = self._user.id
            task.critted_by_name = self._user.display_name
            store_tasks._update(task)

        store_taste.record(
            score=score,
            reason=reason,
            task_id=task_id,
            change_id=task.change_ids[0] if task.change_ids else None,
        )

        # Trust score flows from crit to submitter
        if task.submitted_by:
            delta = score * 0.1
            try:
                store_users.update_trust(task.submitted_by, delta)
            except ValueError:
                pass  # submitter no longer exists

        # Promote or discard staged files
        ws = Workspace(ROOT)
        file_scope = task.scope.split(":")[0] if ":" in task.scope else task.scope
        if score > 0:
            promoted = ws.promote_staged(file_scope)
            if promoted:
                _deploy_site()
        else:
            ws.discard_staged(file_scope)

        self._json({"ok": True, "score": score})

    def _serve_static(self):
        path = self.path.rstrip("/") or "/index.html"
        if path == "/":
            path = "/index.html"
        filepath = SITE / path.lstrip("/")

        if not filepath.resolve().is_relative_to(SITE.resolve()):
            self.send_response(403)
            self.end_headers()
            return

        if not filepath.exists():
            # Try .html extension (for /register → register.html, etc.)
            html_path = SITE / (path.lstrip("/") + ".html")
            if html_path.exists() and html_path.resolve().is_relative_to(SITE.resolve()):
                filepath = html_path
            else:
                filepath = SITE / "index.html"

        content_types = {
            ".html": "text/html",
            ".css": "text/css",
            ".js": "application/javascript",
            ".json": "application/json",
            ".png": "image/png",
            ".svg": "image/svg+xml",
        }
        ext = filepath.suffix.lower()
        ct = content_types.get(ext, "application/octet-stream")

        self.send_response(200)
        self.send_header("Content-Type", ct)
        self.end_headers()
        self.wfile.write(filepath.read_bytes())


def _run_wright_async(task_id):
    """Run wright in background thread."""
    def work():
        try:
            wright = Wright(ROOT)
            wright.work(task_id)
        except Exception as e:
            print(f"Wright error on {task_id}: {e}")
    Thread(target=work, daemon=True).start()


def _deploy_site():
    """Sync site directory to the web server."""
    import subprocess
    dest = os.environ.get("WW_DEPLOY_HOST", "")
    if not dest:
        return
    web_root = "/var/www/workwright.xyz/html"
    try:
        # rsync all site files — only changed ones transfer
        subprocess.run([
            "rsync", "-az", "--delete",
            str(SITE) + "/",
            f"{dest}:/tmp/ww-site/"
        ], timeout=15, capture_output=True)
        subprocess.run([
            "ssh", dest,
            f"sudo rsync -a /tmp/ww-site/ {web_root}/"
        ], timeout=10, capture_output=True)
    except Exception as e:
        print(f"Deploy error: {e}")


def serve(host="0.0.0.0", port=8077):
    """Start the server."""
    server = HTTPServer((host, port), Handler)
    print(f"Workwright API on http://{host}:{port}")
    server.serve_forever()


if __name__ == "__main__":
    serve()
