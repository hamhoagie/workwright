"""
commands — one function per CLI verb.
"""

import json
import os
import sys

from src.workspace import Workspace
from src.task import TaskStore, TaskStatus
from src.evaluate import evaluate_file
from src.taste import TasteStore
from src.wright import Wright
from src.users import UserStore


def cmd_init(args):
    ws = Workspace(args.path)
    TaskStore(args.path)
    TasteStore(args.path)
    print(f"Initialized workspace at {args.path}")
    print(f"  .workwright/ created with locks, changelog, tasks, taste stores")


def cmd_task(args):
    store = TaskStore(args.path)
    task = store.create(intent=args.intent, why=args.why, scope=args.scope,
                        context=args.context)
    print(f"Created task {task.id}: {task.intent}")
    print(f"  Why: {task.why}")
    print(f"  Scope: {task.scope}")
    if task.context:
        print(f"  Context: {', '.join(task.context)}")


def cmd_tasks(args):
    store = TaskStore(args.path)
    tasks = store.all()

    if getattr(args, "json", False):
        print(json.dumps([{
            "id": t.id,
            "intent": t.intent,
            "status": t.status.value,
            "scope": t.scope,
            "taste_score": t.taste_score,
            "taste_note": t.taste_note,
        } for t in tasks], indent=2))
        return

    if not tasks:
        print("No tasks.")
        return
    for t in tasks:
        status_icon = {
            "pending": "⏳", "active": "🔨", "review": "👁",
            "accepted": "✅", "rejected": "❌", "abandoned": "💀"
        }.get(t.status.value, "?")
        print(f"  {status_icon} {t.id}  {t.intent}")
        if t.taste_score is not None:
            print(f"         taste: {t.taste_score:+.1f}  {t.taste_note or ''}")


def cmd_review(args):
    store = TaskStore(args.path)
    tasks = store.for_review()
    if not tasks:
        print("Nothing to review.")
        return
    print(f"{len(tasks)} task(s) awaiting review:\n")
    for t in tasks:
        print(f"  {t.id}  {t.intent}")
        print(f"         scope: {t.scope}")
        if t.change_ids:
            print(f"         changes: {', '.join(t.change_ids)}")
        print()


def cmd_evaluate(args):
    task_store = TaskStore(args.path)
    taste_store = TasteStore(args.path)

    task = task_store.get(args.task_id)
    if not task:
        print(f"Task {args.task_id} not found")
        sys.exit(1)

    score = float(args.score)
    reason = " ".join(args.reason) if args.reason else ""

    task_store.evaluate(args.task_id, score, reason)
    taste_store.record(
        score=score, reason=reason,
        task_id=args.task_id,
        change_id=task.change_ids[0] if task.change_ids else None,
    )

    verdict = "accepted ✅" if score > 0 else "rejected ❌"
    print(f"Task {args.task_id} {verdict} (score: {score:+.1f})")
    if reason:
        print(f"  Reason: {reason}")


def cmd_taste(args):
    taste = TasteStore(args.path)

    if getattr(args, "json", False):
        print(json.dumps(taste.patterns(), indent=2))
        return

    print(taste.guide())


def cmd_eval_file(args):
    result = evaluate_file(args.file)
    print(f"File: {result.path}")
    print(f"Score: {result.score:.2f}")
    print(f"  Single responsibility: {'✅' if result.single_responsibility else '❌'}")
    print(f"  Readable:              {'✅' if result.readable else '❌'}")
    print(f"  Concise:               {'✅' if result.concise else '❌'}")
    if result.issues:
        print(f"\nIssues:")
        for issue in result.issues:
            print(f"  - {issue}")
    if result.suggestion:
        print(f"\nSuggestion: {result.suggestion}")


def cmd_changes(args):
    ws = Workspace(args.path)
    changes = ws.recent_changes(limit=args.limit)
    if not changes:
        print("No changes recorded.")
        return
    for c in changes:
        taste = f" [{c.taste_score:+.1f}]" if c.taste_score is not None else ""
        print(f"  {c.id}  {c.path}  ({c.agent_id}){taste}")
        print(f"         {c.intent}")


def cmd_locks(args):
    ws = Workspace(args.path)
    locks = ws.list_locks()
    if not locks:
        print("No active locks.")
        return
    for path, lock in locks.items():
        print(f"  🔒 {path}  ({lock['agent_id']})")
        print(f"      {lock['intent']}")


def cmd_status(args):
    task_store = TaskStore(args.path)
    taste_store = TasteStore(args.path)
    ws = Workspace(args.path)

    tasks = task_store.all()
    locks = ws.list_locks()
    changes = ws.recent_changes(limit=5)
    patterns = taste_store.patterns()

    counts = {s: sum(1 for t in tasks if t.status == s) for s in TaskStatus}
    print(f"Workspace: {args.path}")
    print(f"Tasks: {counts[TaskStatus.PENDING]} pending, {counts[TaskStatus.ACTIVE]} active, "
          f"{counts[TaskStatus.REVIEW]} review, {counts[TaskStatus.ACCEPTED]} accepted, "
          f"{counts[TaskStatus.REJECTED]} rejected")
    print(f"Locks: {len(locks)} active")
    print(f"Taste signals: {patterns.get('signal_count', 0)}")

    if changes:
        print(f"\nRecent changes:")
        for c in changes[:3]:
            print(f"  {c.id}  {c.path} — {c.intent}")


def cmd_run(args):
    wright = Wright(args.path)
    result = wright.work(args.task_id)
    if result.success:
        print(f"✅ Task {result.task_id} completed")
        print(f"   Files: {', '.join(result.files_changed)}")
        print(f"   Self-eval: {result.evaluation_score:.2f}")
        if result.defense:
            print(f"\n   Defense: {result.defense}")
        print(f"\n   Ready for review: ww evaluate {result.task_id} <score> [reason]")
    else:
        print(f"❌ Task {result.task_id} failed: {result.message}")


def cmd_run_next(args):
    wright = Wright(args.path)
    result = wright.work_next()
    if result is None:
        print("No pending tasks.")
        return
    if result.success:
        print(f"✅ Task {result.task_id} completed")
        print(f"   Files: {', '.join(result.files_changed)}")
        print(f"   Self-eval: {result.evaluation_score:.2f}")
        if result.defense:
            print(f"\n   Defense: {result.defense}")
        print(f"\n   Ready for review: ww evaluate {result.task_id} <score> [reason]")
    else:
        print(f"❌ Task {result.task_id} failed: {result.message}")


def cmd_register(args):
    """Register a new participant and print their token."""
    store = UserStore(args.path)
    try:
        user = store.register(args.email, args.display_name)
    except ValueError as e:
        print(f"Error: {e}")
        sys.exit(1)
    print(f"Registered: {user.display_name} ({user.email})")
    print(f"  ID:    {user.id}")
    print(f"  Token: {user.token}")
    print(f"\nSet WW_TOKEN={user.token} to use this token with ww me")


def cmd_users(args):
    """List all participants with trust scores."""
    store = UserStore(args.path)
    users = store.all()
    if not users:
        print("No users.")
        return
    print(f"{'NAME':<20} {'ROLE':<12} {'TRUST':>6}  {'ID'}")
    print("-" * 50)
    for u in users:
        bar = "█" * int(u.trust_score * 10) + "░" * (10 - int(u.trust_score * 10))
        print(f"  {u.display_name:<18} {u.role.value:<12} {u.trust_score:>5.2f}  {u.id}  {bar}")


def cmd_me(args):
    """Show own profile (requires WW_TOKEN env var)."""
    token = os.environ.get("WW_TOKEN", "")
    if not token:
        print("WW_TOKEN not set. Export your token: export WW_TOKEN=<your-token>")
        sys.exit(1)
    store = UserStore(args.path)
    user = store.get_by_token(token)
    if not user:
        print("Token not recognized.")
        sys.exit(1)
    print(f"Name:        {user.display_name}")
    print(f"Email:       {user.email}")
    print(f"ID:          {user.id}")
    print(f"Role:        {user.role.value}")
    print(f"Trust score: {user.trust_score:.2f}")