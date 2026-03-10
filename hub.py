#!/usr/bin/env python3
"""
hub — the CLI interface.

One tool, clear verbs. Unix style.

Usage:
    hub init <path>                     Initialize a workspace
    hub task <intent> --why <why> --scope <scope>   Create a task
    hub tasks                           List all tasks
    hub review                          Show tasks awaiting review
    hub evaluate <task_id> <score> [reason]   Record taste signal
    hub taste                           Show learned taste guide
    hub eval-file <path>                Evaluate a file against principles
    hub changes                         Show recent changes
    hub locks                           Show active locks
    hub status                          Workspace overview
"""

import argparse
import sys
import json
from pathlib import Path

from src.workspace import Workspace
from src.task import TaskStore, TaskStatus
from src.evaluate import Evaluator
from src.taste import TasteStore


def cmd_init(args):
    ws = Workspace(args.path)
    TaskStore(args.path)
    TasteStore(args.path)
    print(f"Initialized workspace at {args.path}")
    print(f"  .agenthub/ created with locks, changelog, tasks, taste stores")


def cmd_task(args):
    store = TaskStore(args.path)
    task = store.create(intent=args.intent, why=args.why, scope=args.scope)
    print(f"Created task {task.id}: {task.intent}")
    print(f"  Why: {task.why}")
    print(f"  Scope: {task.scope}")


def cmd_tasks(args):
    store = TaskStore(args.path)
    tasks = store.all()
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

    # Record in both places
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
    guide = taste.guide()
    print(guide)


def cmd_eval_file(args):
    evaluator = Evaluator()
    result = evaluator.evaluate_file(args.file)
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

    pending = sum(1 for t in tasks if t.status == TaskStatus.PENDING)
    active = sum(1 for t in tasks if t.status == TaskStatus.ACTIVE)
    review = sum(1 for t in tasks if t.status == TaskStatus.REVIEW)
    accepted = sum(1 for t in tasks if t.status == TaskStatus.ACCEPTED)
    rejected = sum(1 for t in tasks if t.status == TaskStatus.REJECTED)

    print(f"Workspace: {args.path}")
    print(f"Tasks: {pending} pending, {active} active, {review} review, "
          f"{accepted} accepted, {rejected} rejected")
    print(f"Locks: {len(locks)} active")
    print(f"Taste signals: {patterns.get('signal_count', 0)}")

    if changes:
        print(f"\nRecent changes:")
        for c in changes[:3]:
            print(f"  {c.id}  {c.path} — {c.intent}")


def main():
    parser = argparse.ArgumentParser(
        prog="hub",
        description="Agency for all things. Humans above the loop.",
    )
    parser.add_argument("--path", default=".", help="Workspace path")
    sub = parser.add_subparsers(dest="command", required=True)

    # init
    p = sub.add_parser("init", help="Initialize a workspace")
    p.add_argument("init_path", nargs="?", default=".")
    p.set_defaults(func=lambda a: cmd_init(type(a)(**{**vars(a), "path": a.init_path})))

    # task
    p = sub.add_parser("task", help="Create a task")
    p.add_argument("intent", help="What needs to happen")
    p.add_argument("--why", required=True, help="Why it matters")
    p.add_argument("--scope", required=True, help="File or function this touches")
    p.set_defaults(func=cmd_task)

    # tasks
    p = sub.add_parser("tasks", help="List all tasks")
    p.set_defaults(func=cmd_tasks)

    # review
    p = sub.add_parser("review", help="Tasks awaiting review")
    p.set_defaults(func=cmd_review)

    # evaluate
    p = sub.add_parser("evaluate", help="Record taste signal")
    p.add_argument("task_id", help="Task to evaluate")
    p.add_argument("score", type=float, help="Score: -1.0 to 1.0")
    p.add_argument("reason", nargs="*", help="Why (taste signal)")
    p.set_defaults(func=cmd_evaluate)

    # taste
    p = sub.add_parser("taste", help="Show learned taste guide")
    p.set_defaults(func=cmd_taste)

    # eval-file
    p = sub.add_parser("eval-file", help="Evaluate a file against principles")
    p.add_argument("file", help="Path to file")
    p.set_defaults(func=cmd_eval_file)

    # changes
    p = sub.add_parser("changes", help="Recent changes")
    p.add_argument("--limit", type=int, default=20)
    p.set_defaults(func=cmd_changes)

    # locks
    p = sub.add_parser("locks", help="Active locks")
    p.set_defaults(func=cmd_locks)

    # status
    p = sub.add_parser("status", help="Workspace overview")
    p.set_defaults(func=cmd_status)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
