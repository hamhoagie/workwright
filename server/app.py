"""
Workwright API — serves the live workspace.

Four endpoints. Nothing extra.
"""

import json
import os
import time
import hashlib
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler
from threading import Thread

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


store_tasks = TaskStore(ROOT)
store_taste = TasteStore(ROOT)


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
    }


def _read_body(handler):
    """Read and parse JSON body."""
    length = int(handler.headers.get("Content-Length", 0))
    raw = handler.rfile.read(length)
    return json.loads(raw) if raw else {}


def _rate_key(handler):
    """Rate limit key from IP."""
    addr = handler.client_address[0]
    forwarded = handler.headers.get("X-Forwarded-For")
    if forwarded:
        addr = forwarded.split(",")[0].strip()
    return addr


# Simple in-memory rate limiter
_rate_counts = {}  # ip -> (count, window_start)
RATE_LIMIT = 10     # tasks per window
RATE_WINDOW = 3600  # 1 hour


def _check_rate(ip):
    """Return True if allowed."""
    now = time.time()
    count, start = _rate_counts.get(ip, (0, now))
    if now - start > RATE_WINDOW:
        _rate_counts[ip] = (1, now)
        return True
    if count >= RATE_LIMIT:
        return False
    _rate_counts[ip] = (count + 1, start)
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
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_OPTIONS(self):
        self._cors()

    def do_GET(self):
        if self.path == "/api/tasks":
            return self._get_tasks()
        if self.path == "/api/taste":
            return self._get_taste()
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

    def _get_preview(self, change_id):
        """Serve the proposed file as a rendered HTML page."""
        ws = Workspace(ROOT)
        changes = ws.recent_changes(limit=100)
        for c in changes:
            if c.id == change_id:
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
                # Read the file at its current state
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

    def _post_task(self):
        if not _check_rate(_rate_key(self)):
            return self._json({"error": "rate limited"}, 429)

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
            scope=scope,              # keep :design tag
            context=[file_path],       # clean path for file ops
        )

        # Wright works on it in background
        _run_wright_async(task.id)

        self._json(_task_json(task), 201)

    def _post_crit(self):
        if not _check_rate(_rate_key(self)):
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

        store_tasks.evaluate(task_id, score, reason)
        store_taste.record(
            score=score,
            reason=reason,
            task_id=task_id,
            change_id=task.change_ids[0] if task.change_ids else None,
        )

        # If accepted, deploy
        if score > 0:
            _deploy_site()

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
    """Copy site to the web server."""
    import subprocess
    try:
        subprocess.run([
            "scp", str(SITE / "index.html"),
            "billy@WW_DEPLOY_HOST:/tmp/ww-index.html"
        ], timeout=10, capture_output=True)
        subprocess.run([
            "ssh", "billy@WW_DEPLOY_HOST",
            "sudo cp /tmp/ww-index.html /var/www/workwright.xyz/html/index.html"
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
