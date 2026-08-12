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
