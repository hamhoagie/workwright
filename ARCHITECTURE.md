# Architecture

## One Protocol

Everything flows through the same loop:

```
task → claim → work → defense → submit → crit → taste signal
                                                      │
                                                      ▼
                                                 taste store
                                                      │
                                                      ▼
                                              taste guide (shared)
```

The loop is the same regardless of who's in it. A wright (LLM), a human, a junior dev, a team. The protocol doesn't care. It cares about: did you do the work, can you defend it, does the crit accept it.

## Participants

A participant is anyone who can claim a task and submit work. Each has:

- **id** — unique identifier
- **type** — `wright` (LLM), `human`, `team`
- **trust** — earned through accepted work, starts at zero
- **taste_alignment** — how well their work matches the accumulated taste (calculated from scores)

```
participants/
  wright-1.json       # LLM participant
  billy.json          # human participant
  junior-dev.json     # human participant
```

### Trust

Trust is a number from 0.0 to 1.0. It determines review frequency:

| Trust | Review | Meaning |
|-------|--------|---------|
| 0.0–0.3 | Every submission | New. Prove yourself. |
| 0.3–0.6 | Most submissions | Learning. Getting there. |
| 0.6–0.8 | Spot checks | Reliable. Earned autonomy. |
| 0.8–1.0 | Exceptions only | Trusted. Human intervenes when curious. |

Trust increases with accepted work. Decreases with rejections. Decays slowly over inactivity. Same math for wrights and humans.

A wright starts at 0.0 every time. A human might start higher based on prior work. Trust is earned, not configured.

## Tasks

A task is the atomic unit. One intent, one scope, one why.

```json
{
  "id": "a3f7c2",
  "intent": "extract validation logic into its own module",
  "why": "validation is a concern independent of the handler — coupling them means changing validation rules requires touching request handling",
  "scope": "src/validate.py",
  "context": ["src/handler.py", "src/types.py"],
  "created_by": "billy",
  "claimed_by": null,
  "status": "pending"
}
```

Tasks come from humans (top-down intent) or from the system (evaluator flags a problem). A wright can propose tasks but can't create them without human approval — that's a trust boundary.

### Task Size

The atomic element is task-relative: the smallest thing that can be evaluated against the why. Not a fixed unit. A task scoped to a function is fine. A task scoped to a module is fine. A task scoped to "refactor the whole backend" is too big to crit — break it down.

The rule: if you can't read the diff, read the defense, and give a score in under two minutes, the task is too big.

## Defense

Every submission includes a defense. Not optional. Not a commit message. A conceptual argument for why the work takes this form.

```json
{
  "task_id": "a3f7c2",
  "participant": "wright-1",
  "defense": "Validation is a predicate on input shape, not a step in request handling. Separating it means validation rules can be tested, composed, and reused without importing the HTTP layer. The module exposes pure functions that return (bool, error_message) — no side effects, no framework dependency.",
  "files_changed": ["src/validate.py"],
  "self_eval": 0.85
}
```

The defense answers: **why this form and not another.** Not what you did — the diff shows that. Why it's right.

A defense that says "I moved the functions because the task said to" is a zero. That's execution without understanding. The taste store will punish it.

### Defense for Humans Too

When a human submits through `ww submit`, they write a defense. Same format, same expectation. The system doesn't know or care that you're human. Defend your choices.

This is the Pratt principle: you pin your work to the wall and explain why it exists in this form. Whether you're a first-year student or a tenured professor.

## Crit

Crit is evaluation with dialogue. The reviewer reads the diff and the defense, then scores.

```
ww crit a3f7c2
```

Shows:
1. The task (intent + why)
2. The diff
3. The defense
4. Prompt: score (-1.0 to 1.0) and reason

The score and reason become a taste signal. The reason is the important part — it's what the taste store learns from.

### Who Crits Whom

| Submitter | Default Reviewer | Escalation |
|-----------|-----------------|------------|
| Wright (low trust) | Human | — |
| Wright (high trust) | Auto-accept | Human spot-check |
| Human (low trust) | Senior human | — |
| Human (high trust) | Self-accept | Peer spot-check |
| Any (contested) | Human panel | — |

At the start, humans crit everything. As trust builds, the system relaxes. This is how a studio works.

## Taste Store

The taste store accumulates signals from every crit:

```json
{
  "score": 0.8,
  "reason": "Clean separation. Defense holds — validation really is independent of handling. Pure functions are the right call here.",
  "task_id": "a3f7c2",
  "reviewer": "billy",
  "participant": "wright-1",
  "timestamp": 1710115200
}
```

Over time, the store builds patterns:
- What gets accepted (and why)
- What gets rejected (and why)
- Which defenses hold up
- Which participants are improving

### Taste Guide

Generated from the store. Read by every participant before starting work.

The guide isn't rules. It's accumulated crit — distilled into principles that reflect the reviewer's actual taste, not their stated preferences. What you accept reveals what you value. The guide reflects that.

```
## Taste Guide (generated from 47 signals)

### Principles (from accepted work)
- Separate concerns by concept, not by layer
- Pure functions over stateful methods
- Name things for what they are, not what they do

### Anti-patterns (from rejected work)  
- God functions that handle multiple concerns
- Defenses that describe what instead of why
- Coupling that forces unrelated files to change together

### Emerging patterns
- Reviewer values conceptual clarity over cleverness
- Short functions accepted 3:1 over long ones
- Defenses citing independence/composability score highest
```

## Workspace

The shared surface where work happens. Files, locks, changelog.

```
project/
  .workwright/
    participants/       # who's in the workspace
    tasks.jsonl         # all tasks
    taste.jsonl         # all taste signals
    taste_guide.md      # generated guide (rebuilt on new signals)
    changelog.jsonl     # every file change with intent
    locks.json          # who's working on what
    trust.json          # participant trust scores
  src/                  # the actual code
  ...
```

### Locks

Fine-grained. A participant locks the files they're working on. Others can see the intent and coordinate. No two participants work on the same file simultaneously.

### Changelog

Every change is recorded with who, what, why, and the defense. The changelog is a crit history, not a commit log.

## CLI

Same interface for everyone:

```bash
# Workspace
ww init .                          # initialize
ww status                          # overview

# Tasks
ww task "intent" --why "reason" --scope file.py --context other.py
ww tasks                           # list all
ww claim <task_id>                 # claim a task (human)

# Work
ww submit --defense "why this form" # submit work with defense
ww run <task_id>                    # wright claims + works + submits
ww run-next                         # wright picks next pending

# Crit  
ww crit <task_id>                  # review: see diff + defense, score it
ww review                          # list tasks awaiting crit

# Taste
ww taste                           # show current taste guide
ww taste-history                   # all signals

# Evaluation
ww eval-file <path>                # evaluate a file against principles

# Participants
ww join --type human --id billy    # join the workspace
ww trust                           # show trust levels
ww who                             # who's working on what
```

## What This Is Not

- **Not a CI/CD pipeline.** No deploy step. No environments. Just crit.
- **Not a code review tool.** Code review asks "does this look okay." Crit asks "should this exist in this form."
- **Not project management.** No sprints, no velocity, no story points. Tasks and taste.
- **Not an AI coding assistant.** The wright is a participant, not the product. The protocol is the product.

## What This Is

A crit system. A workshop protocol. A way to transmit taste — from humans to wrights, from seniors to juniors, from the accumulated history of judgment to the next piece of work.

The protocol doesn't care if you're an LLM or a person. It cares if you can defend your work.

---

*March 2026*
