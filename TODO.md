# Workwright — Build Plan

## Day 1 (Mar 11) — Make It Real

### 1. Refactor own code (dogfooding)
The evaluator flagged taste.py and evaluate.py for long functions.
Fix them first. Prove the system works on itself.

### 2. Wright integration
Wire in an actual coding agent (wright) that can:
- Pick up a pending task from the queue
- Read the taste guide
- Lock the relevant files
- Do the work
- Submit for review
- The wright should be an LLM call (Sonnet for speed, Opus for quality)

### 3. Taste guide injection
When a wright picks up a task, it receives:
- The task intent + why
- The current taste guide (learned from signals so far)
- The scope (file contents)
- The two questions as evaluation criteria

### 4. Human review flow
After a wright submits:
- Show the diff (what changed)
- Show it as a single evaluable unit
- Human scores + gives reason
- Signal feeds back into taste store
- Wright gets smarter next time

### 5. Test on real code
Use workwright itself as the test codebase.
Create tasks, let wrights execute, evaluate results.
Recursive self-improvement from day one.

## Design Decisions Still Open

- **Task-relative atomic sizing:** How does the wright propose the right unit size?
  Current: human defines scope. Future: wright proposes, human taste-signals the scoping.
  
- **Multi-wright coordination:** When two wrights work on related files.
  Current: file-level locking prevents collisions. Future: intent-aware coordination.

- **Taste model depth:** Current is keyword extraction + counters.
  Next: LLM-summarized taste patterns. Eventually: embedding-based similarity
  (use V-JEPA-style representation learning on code preferences).

- **The name for a unit of work:** "task" is generic. Something better?

## Architecture Notes

- Everything is files (JSONL stores, not databases)
- Each module does one thing
- CLI is the interface (ww.py)
- No framework dependencies
- Python stdlib + LLM API calls only

## v2 — Rust Rewrite
- [ ] Rewrite core in Rust
- [ ] User registration + login (email or GitHub OAuth)
- [ ] Per-user tokens, revocable
- [ ] Identity on every brief and crit (who said what)
- [ ] Trust scores per participant (earned through accepted work)
- [ ] Taste guide weighted by reviewer trust
- [ ] Wright writes to staging, never live — deploy only on accepted crit
- [ ] Multi-file scope for briefs
- [ ] Persistent rate limiting (not in-memory)
