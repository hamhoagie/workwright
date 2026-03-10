# AgentHub — Agency for All Things

> Humans above the loop, not in it.

## The Problem

Current agent-code workflows are stuck between two bad options:
1. **Full autonomy** — agents work alone, produce mid work, nobody's steering
2. **Human-in-the-loop** — human reviews every change, bottlenecks everything

Both miss the point. The bottleneck isn't speed or review — it's **taste**. Knowing what "good" looks like. Setting the conditions for good work to happen, then getting out of the way.

## The Philosophy

From dog training, farm management, aikido, and workspace architecture:
- **Create the conditions for things to flourish, then step back**
- **Agency for all things** — agents are autonomous within a container humans define
- **Taste as gradient signal** — humans don't review diffs, they shape what "good" means
- **The process is the point** — not control, not abdication. Stewardship.

## What We're Building

A workspace where:
- **Humans set intent and taste** — what are we building, what does good look like, what are the constraints
- **Agents work autonomously** — fast, parallel, within the container
- **Continuous evaluation** replaces code review — does it work? is it better?
- **Taste learning** — the system learns what "good" means for *this human* from their reactions over time
- **Humans check in when they want** — not when the system demands it

## What We're NOT Building

- Another CI/CD pipeline
- A code review tool
- An "AI coding assistant"
- A project management layer
- Git with extra steps

## Architecture (Draft)

```
┌─────────────────────────────────┐
│         TASTE LAYER             │
│   Human intent, preferences,    │
│   learned sensibility           │
│   (above the loop)              │
└──────────────┬──────────────────┘
               │ defines "good"
               ▼
┌─────────────────────────────────┐
│         ORCHESTRATOR            │
│   Assigns work, prevents        │
│   collisions, routes results    │
└──────────────┬──────────────────┘
               │ coordinates
               ▼
┌─────────────────────────────────┐
│       SHARED WORKSPACE          │
│   Fine-grained locking          │
│   (function/block level)        │
│   Live state, not history       │
│   Intent declarations           │
└──────────────┬──────────────────┘
               │ validates
               ▼
┌─────────────────────────────────┐
│       CONTINUOUS EVALUATOR      │
│   Tests, benchmarks, regression │
│   Taste model (learned)         │
│   Fitness scoring per change    │
└─────────────────────────────────┘
```

## First Principles

### Unix Philosophy as Architecture
Not a style preference — a design constraint that makes everything else work:
- **Files** — the universal interface. Not databases, not proprietary formats.
- **Single functions** — do one thing well. Not god-objects, not frameworks.
- **Readable** — code is for humans to understand and machines to execute.
- **Concise** — say what you mean, nothing more.

This isn't aesthetics. It's structural. Small units are evaluable. Large ones aren't.

### The Two Questions
Every piece of work gets evaluated against:
1. **Why are we making this?** — does it serve the purpose?
2. **How does it solve it elegantly?** — nothing extra, nothing missing?

These are the same questions a good creative director asks. They're the fitness function.

### The Right-Sized Loop
Human-in-the-loop doesn't fail because humans are slow. It fails because humans are asked to evaluate too much at once. A 47-file PR is unevaluable. Nobody has taste at that resolution.

A single thing — one function, one file, one clear intent — a human can evaluate in seconds. Yes, this is right. No, this is wrong, and here's why. That's a meaningful taste signal.

**The architecture produces work in units that match the grain of human judgment.**

The agent does the volume. The human does the taste. But only because the work arrives in pieces small enough to taste.

### Intent, Not Diffs
Agents declare what they're trying to accomplish, not just what they changed. Other agents (and humans) see the intent and can coordinate or redirect.

### Taste as Learned Model
Not a style guide. Not linting rules. A model that learns from human reactions:
- You rejected this pattern three times → the model learns
- You praised this approach → the model learns
- Over time, agents produce work that fits *your* sensibility

Same principle as training a world model on camera feeds (V-JEPA) — build a representation of "good" through exposure, not explicit labels. The taste model is a V-JEPA for code quality.

### Stewardship, Not Control
The human's job:
- Define why we're building (high-level intent)
- React to results (taste signal)
- Set boundaries that matter (safety, style, scope)
- Check in when curious

NOT the human's job:
- Review every change
- Approve every action
- Manage the process
- Be the bottleneck

## MVP Scope

Start concrete, stay small:
1. **Shared workspace** with lock table (function-level)
2. **Orchestrator** that assigns tasks from a goal list
3. **Evaluator** that runs tests + basic quality scoring
4. **Taste store** that records human reactions (accept/reject/modify + why)
5. **Test it** on a real codebase (clawdius.dev or a new project)

## Name

AgentHub is Karpathy's. We need our own.

Ideas:
- **Grove** (things grow, you tend them)
- **Paddock** (bounded space for free movement — the Otto principle)
- **Steward** (the role, not the tool)
- **Dojo** (practice space — the aikido thread)

Open question.

---

*Billy & Clawdius, March 2026*
