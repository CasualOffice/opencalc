# CLAUDE.md

This repository is governed by [AGENTS.md](AGENTS.md). Read it first — it is the
full contract. This file only restates the essentials.

- **Design first, then execute.** Get the layer division and virtualization right
  the first time; do not defer design or plan for do-overs.
- **Every task runs the same cycle**, and none of it is optional:

      design → execute → check → find issues → fix → push, with tests

  **Check** means running the thing, not re-reading it. This project's expensive
  bugs have all been invisible to reading — a wire format that serialized
  perfectly and could not be read back, a submission the server silently
  dropped, an autofit that measured rotated text as flat. Every one was found by
  running something new.

  **Find issues** is a step, not an outcome. A change that passes first time has
  usually been checked too narrowly: the question to ask is what would have to
  be true for this to be wrong, and then to go and look.

  **With tests** means a test that fails without the fix. A test written after a
  green run, never seen red, is a test that asserts the code does what it does.
  Prove it by breaking the fix and watching the test catch it.
- **Discuss and finalize** substantial designs before implementing.
- **Track everything.** Every unit of work has a row in
  [docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md), created when it
  starts and updated as its status moves. No untracked work.
- **Production-grade baseline.** Correctness, determinism, security bounds, and
  fidelity come before performance, and performance is a designed-in target
  (1M cells / 60 fps / <50 ms recalc), not an afterthought.
- **No silent data loss**; the calc engine is held back but fully designed.

Current state: **alpha — engine, editor and embeddable SDK are live.** Phases
0–1E done, Phase 2 (calc) substantially done, Phase 3 shipped; the SDK is built
but unpublished, and the collaboration concurrency model is still undecided.
See AGENTS.md §"Current state".

## Focus mode

When the user says **"focus mode"** (or "ultracode"), work this way until told
otherwise. It is one long-lived orchestrator plus disposable workers.

**The orchestrator is this session.** It holds no work in its head: the truth is
[docs/14-EXECUTION-TRACKER.md](docs/14-EXECUTION-TRACKER.md), re-read rather than
remembered, so the mode survives compaction and picks up in a new session from
the tracker alone. Its loop is: pick the next rows → brief workers → wait →
**verify** → write what happened into the tracker → pick again.

**Workers are always fresh, never reused.** A worker gets a file map, one job,
and the acceptance test — nothing about previous tasks. Reuse sounds cheaper and
is not: a worker carrying unrelated history judges worse and costs more every
round. Everything reuse seems to buy comes from a better brief.

**How many run at once is decided by invariants, not by a number.** Two workers
may run together only when they cannot touch the same invariant — the rule
[docs/67](docs/67-REPOSITORY-REMEDIATION-PLAN.md) already states for waves. Two
tasks in one file is one worker, in sequence. A read-only audit can be seven.
When in doubt, fewer: a merge conflict between two agents costs more than the
parallelism saved.

**A worker's report is not evidence — and the orchestrator checks it itself.**
This is the rule the mode exists to enforce, and the one that is expensive to
skip:

- Every worker returns a **test that was seen to fail without its fix**, plus
  the verbatim failure output. "Added a test and it passes" is not that.
- **The orchestrator verifies by running, not by spawning a checker.** Revert
  the fix, watch the test go red, restore, re-run. That is a few commands and
  costs almost nothing; a checking *agent* costs as much as the fix did, has to
  rediscover context the orchestrator already has, and is worse at it — it has
  to guess what to revert, where the orchestrator knows. Spend agents on work
  that needs judgement; spend the orchestrator on proof.
- Workers do not edit outside their brief. `git status --porcelain` after every
  round; strays have happened.
- A verification pass that refutes nothing has not been done. If every finding
  survives, distrust the verifier before trusting the findings.

**Spend tokens where they change the answer.** Effort `high` for work that is
genuinely open-ended (a design, an audit, a defect whose mechanism is unknown);
lower for a well-specified change. Do not ask a worker to run the full-workspace
lint — the orchestrator runs the gates once for the whole round. One capable
worker beats three that overlap.

**Between rounds, write down what was learned, not just what was done.** A
tracker row records the defect, the mechanism, the gate, and what made it
invisible — that last part is what stops the same class recurring. Findings that
are real but out of scope become new rows immediately; nothing lives only in a
conversation.
