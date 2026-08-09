# CLAUDE.md

This repository is governed by [AGENTS.md](AGENTS.md). Read it first — it is the
full contract. This file only restates the essentials.

- **Design first, then execute.** Get the layer division and virtualization right
  the first time; do not defer design or plan for do-overs.
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
