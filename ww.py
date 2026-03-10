#!/usr/bin/env python3
"""
ww — workwright CLI.

Humans above the loop, not in it.
"""

import argparse
from src.commands import (
    cmd_init, cmd_task, cmd_tasks, cmd_review, cmd_evaluate,
    cmd_taste, cmd_eval_file, cmd_changes, cmd_locks, cmd_status,
    cmd_run, cmd_run_next,
)


def main():
    parser = argparse.ArgumentParser(
        prog="ww",
        description="Workwright — agency for all things.",
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
    p.add_argument("--context", nargs="*", default=[], help="Related files wright should see")
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

    # run
    p = sub.add_parser("run", help="Wright works on a specific task")
    p.add_argument("task_id", help="Task ID to work on")
    p.set_defaults(func=cmd_run)

    # run-next
    p = sub.add_parser("run-next", help="Wright picks up next pending task")
    p.set_defaults(func=cmd_run_next)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
