# Workwright

**Humans above the loop, not in it.**

Workwright is a protocol for building software and design with AI. Three words:

**Brief → Defense → Crit**

1. You **brief** a wright — what you want and why it matters
2. The wright **defends** its choices — why this form and not another
3. You **crit** the defense — your judgment becomes the taste guide

The system learns what you actually value from your crits, not from what you say you value. Over time, wrights produce work that reflects accumulated taste.

## Live demo

[workwright.xyz](https://workwright.xyz) — a live workspace built through its own protocol. Every change on the site went through brief → defense → crit. The feed is the history.

## What's a wright?

Not an "agent." A wright. From Old English *wyrhta* — one who works. Shipwright, wheelwright, playwright. A craftsperson who works within a tradition.

A wright that can't defend its choices — regardless of whether the code works — has produced accidental work.

## The two questions

Every piece of work must answer:

1. **Why are we making this?**
2. **How does it solve it elegantly?**

These are Socratic. They recurse. The wright answers them before writing a line of code. The human asks them again during crit.

## The protocol

The product is the protocol, not the implementation. Anyone could rewrite this in any language and it'd still be Workwright if it follows:

- **Brief** — human intent with a why
- **Defense** — conceptual argument for the form, not a commit message
- **Crit** — judgment on the defense, not just the output
- **Taste guide** — generated from accumulated crit, not written by hand
- **Trust gradient** — earned through accepted work, never configured
- **Public read, gated write** — anyone can see the work, contributing requires trust

## CLI

```bash
ww init .                    # initialize a workspace
ww task "intent" --why "reason" --scope file.py   # brief
ww run <task_id>             # wright works + defends
ww crit <task_id>            # human judges
ww taste                     # show the learned taste guide
ww eval-file <path>          # evaluate against principles
ww status                    # workspace overview
```

## Architecture

```
brief → wright works → defense → crit → taste signal → taste guide
                                                            ↓
                                                    shapes next brief...
```

See [ARCHITECTURE.md](ARCHITECTURE.md) and [VISION.md](VISION.md).

## What this is not

- Not a CI/CD pipeline
- Not a code review tool
- Not an "AI coding assistant"
- Not project management

It's a crit system. A protocol for transmitting taste.

## Who

Built by **Billy McDermott** ([billy@billy.wtf](mailto:billy@billy.wtf)) and **Clawdius** ([goose@clawdius.dev](mailto:goose@clawdius.dev)) in a single night, March 2026.

---

*The protocol is the product.*
