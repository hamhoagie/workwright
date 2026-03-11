# Workwright

**Humans above the loop, not in it.**

Workwright is a crit system for building software and design with AI. The loop is three words:

**Brief → Defense → Crit**

You brief a wright. The wright does the work and defends its choices — why this form and not another. You crit the defense. Your judgment becomes the taste guide. Over time, the work gets better because the system learns what you actually value, not what you say you value.

## Live demo

[workwright.xyz](https://workwright.xyz) is a live workspace built through its own protocol. Propose a change, a wright does it, defends it, and the community crits the result. Accepted changes deploy live.

## The two questions

Every piece of work must answer:

1. **Why are we making this?**
2. **How does it solve it elegantly?**

These are Socratic. They recurse. The wright answers them before writing a line of code. The human asks them again during crit.

## What's a wright?

Not an "agent." A wright. From Old English *wyrhta* — one who works. Shipwright, wheelwright, playwright. A craftsperson who works within a tradition, not a contractor executing instructions.

A wright that can't defend its choices — regardless of whether the code works — has produced accidental work.

## CLI

```bash
ww init .                    # initialize a workspace
ww task "intent" --why "reason" --scope file.py   # create a task
ww run <task_id>             # wright picks up and works
ww run-next                  # wright picks next pending task
ww crit <task_id>            # review: diff + defense, score it
ww taste                     # show the learned taste guide
ww eval-file <path>          # evaluate a file against principles
ww status                    # workspace overview
```

## How it works

1. A human creates a **brief** — what needs to happen and why
2. A **wright** (LLM) claims the task, reads the taste guide, does the work
3. The wright writes a **defense** — why it made these specific choices, conceptually
4. A human **crits** the defense alongside the diff — accepts or rejects with a reason
5. The reason becomes a **taste signal** that shapes the guide for future work
6. Trust builds through accepted work — more trust means less review

The protocol works for wrights (LLMs), humans, and teams. Same loop, same defense, same crit. The system doesn't care what you are. It cares if you can defend your work.

## Architecture

```
brief (human intent)
  → wright works (within taste guide)
    → defense (why this form)
      → crit (human judgment)
        → taste signal
          → taste guide (accumulated judgment)
            → shapes next brief...
```

See [ARCHITECTURE.md](ARCHITECTURE.md) and [VISION.md](VISION.md) for the full design.

## What this is not

- Not a CI/CD pipeline
- Not a code review tool
- Not an "AI coding assistant"
- Not project management

It's a crit system. A way to transmit taste through repeated judgment on specific work, in units small enough to actually evaluate.

## Status

Early. Built in a night. The protocol works. The taste guide learns. The [live site](https://workwright.xyz) is proof.

---

*March 2026*
