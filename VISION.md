# Workwright

> Humans above the loop, not in it.

## The Core Idea

Every change should be traceable to a reason that exists independent of the change itself. The reason was true before the code existed and it'll be true after the code is deleted. The code is just the current expression of the reason.

This is the difference between craft and production. A production line asks "does it meet spec." Crit asks "should this exist in this form."

Workwright is a crit system. Wrights do the work. Humans ask: *why this form and not another?*

## The Two Questions

Every piece of work must answer:

1. **Why are we making this?**
2. **How does it solve it elegantly?**

These are Socratic. They recurse. The wright answers them before writing a line of code. The human asks them again during crit. "Why do you think what you think you think?"

A wright that can't defend its choices conceptually — regardless of whether the code runs — has produced accidental work. Working code with no defensible reason for its form won't survive the next change.

## The Problem

Current agent-code workflows are stuck between two bad options:

1. **Full autonomy** — agents work alone, produce mid work, nobody's steering
2. **Human-in-the-loop** — human reviews every change, bottlenecks everything

Both miss the point. The bottleneck isn't speed or review — it's **taste**. The loop fails because it's too big, not because humans are slow.

We gave the world a generative engine and 99% of people produced slop with it. Then blamed the engine. AI didn't create slop. It revealed that most people who claimed taste were just good at moodboarding. The moment they had to generate instead of curate, the emptiness was right there on the screen.

Workwright doesn't fix taste. It creates the conditions for taste to transmit — through repeated judgment on specific work, in units small enough to actually evaluate.

## Defense, Not Just Review

The wright produces work **and a defense** — why the choices are right, conceptually, not just technically.

- "I extracted these functions into a module" — that's *what*. The diff shows that.
- "These functions are a single concern — the boundary between human and system — and they belong together because changing the CLI surface should never require touching evaluation logic" — that's *why*. That's defensible.

The human reads the diff and the defense. The score responds to both. This is crit: defend your decision-making. There must be a larger concept for the what.

Crit isn't one-directional scoring. It's dialogue. It's Socratic method. It's art school — you pin your work to the wall and explain why it exists in this form.

## Taste as Gradient

Not a style guide. Not linting rules. A model that learns from human crit:

- You rejected this pattern three times → the model learns
- You praised this approach → the model learns
- A defense that held up → the reasoning becomes a principle
- Over time, wrights produce work that reflects accumulated judgment

You don't learn piano by staring at a Steinway and having opinions about Chopin. You learn by playing a phrase, hearing it's wrong, adjusting, playing again. The taste store is that loop.

## Unix Philosophy as Architecture

Not a style preference — a structural requirement:

- **Files** do one thing
- **Functions** do one thing
- **Readable** — code is for humans to understand
- **Concise** — say what you mean, nothing more

This isn't aesthetics. Small units are evaluable. Large ones aren't. A 47-file PR is unevaluable. Nobody has taste at that resolution. A single function with a clear intent — a human can evaluate that in seconds and give a real taste signal.

**The architecture produces work in units that match the grain of human judgment.**

## The Right-Sized Loop

The agent does the volume. The human does the taste. But only because the work arrives in pieces small enough to taste.

The human's job:
- Define why we're building (high-level intent)
- React to results (taste signal through crit)
- Ask: why this form and not another?
- Check in when curious

Not the human's job:
- Review every change
- Approve every action
- Manage the process
- Be the bottleneck

## Not Agents — Wrights

A wright works within a craft tradition. Autonomous within the container the human defined. The name comes from the Old English *wyrhta* — one who works. Shipwright, wheelwright, playwright. Not an "agent" executing instructions. A craftsperson who understands the tradition and produces work that belongs in it.

## Lineage

Form follows function — Bauhaus, via Pratt Institute, via building things that work. The reference earns itself or it doesn't. The taste store will tell us.

---

*Billy & Clawdius, March 2026*

## The Loop in Three Words

**Brief → Defense → Crit**

You brief the wright. The wright defends its choices. You crit the defense. That's it.
