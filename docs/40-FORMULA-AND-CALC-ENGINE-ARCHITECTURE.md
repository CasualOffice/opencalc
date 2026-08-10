# 40 — Formula & Calculation Engine Architecture

The calc engine is OpenCalc's largest and riskiest surface. It is **built in
Phase 2 but designed now** — because its shape dictates seams in the model
(Phase 1A) and the layout engine (Phase 1C). "Held back, not un-designed"
([11](11-DESIGN-FIRST-PROCESS.md)) is made real by the reserved model seams in
[22](22-NORMALIZED-SCHEMA.md) and the crate boundaries in
[19](19-WORKSPACE-SCAFFOLD-DESIGN.md).

Two crates split the concern deliberately:

- **`casual-calc-formula`** — the formula *language*: tokenizer, parser, AST,
  reference algebra, pretty-printer. Available from Phase 1A. No evaluation.
- **`casual-calc-eval`** — the *engine*: dependency graph, recalculation, the
  function library. Phase 2. **Nothing depends on it except the host bridges.**

## Runs natively on desktop, as WASM in the browser (same code)

The engine is **host-agnostic and pure Rust**. There is exactly one calc engine;
the *host bridge* differs:

| Host | Bridge | How calc runs |
| --- | --- | --- |
| **Tauri desktop app** | `casual-calc-sdk` used from Tauri commands (optionally a thin `casual-calc-tauri` integration crate) | **Native Rust** — full-speed, multi-threaded, no WASM ceiling |
| **Web app** | `casual-calc-wasm` (`wasm-bindgen`) | **WASM** in the browser, single-threaded (or Web Worker) |
| Headless service | `casual-calc-sdk` directly | Native Rust |

Consequences the design accounts for from the start:

- **No `#[cfg]` forks in the engine.** `casual-calc-eval` compiles unchanged to
  native and `wasm32-unknown-unknown`. Anything that can't (threads, timers,
  SIMD intrinsics, `std::time`) is behind a **host-provided capability trait**
  the bridge implements — native uses real threads/clock; WASM uses a
  single-thread/worker + host clock. The engine never calls the platform
  directly.
- **The T3 budget (<50 ms worst-case recalc, [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md))
  is defined on the WASM path** (the tighter environment). Native (Tauri) has
  more headroom and may enable parallel recalc where the graph allows; both paths
  produce **identical results** (determinism is not traded for parallelism).
- **Determinism is platform-independent.** Same workbook + same edits ⇒ same
  values on desktop and web. Float evaluation follows a fixed, documented order
  so native and WASM agree bit-for-bit where the spec allows.

## `casual-calc-formula` — the language

> **Status: implemented (Phase 1A, P1A-002).** Tokenizer + precedence-climbing
> parser + serializable `Expr` AST + **minimally-bracketed** printer. Subset: literals,
> A1 references with `$` anchors and sheet qualification (`Sheet2!A1`,
> `'My Sheet'!B2`), cell ranges, defined names, function calls, unary `+`/`-`/`%`,
> and binary arithmetic/comparison/concat with correct precedence and
> right-associative `^`. `parse(print(e)) == e` is gated. Import parses `<f>`
> into the AST and stores it in the model's arena. Deferred: R1C1, full row/column
> ranges (`A:A`), 3-D refs, structured (table) references, union/intersection.

### Tokenizer & parser

- Tokenizes Excel formula syntax: numbers, strings, booleans, error literals,
  operators (`+ - * / ^ & = <> < > <= >= %`, unary `-`/`+`, range `:`,
  intersection space, union `,`), function calls, references, and names.
- Produces a **stable AST**. Parsing is error-tolerant enough to preserve a
  malformed formula's text for write-back rather than losing it.
- Round-trips: `parse → print → parse` is a fixed point (a Phase 1A gate), and
  `parse → print` is the identity on text that is already minimally bracketed
  (FID-05). The second matters because a cell stores the *tree*, not the text
  that was typed: the formula bar shows what the printer emits, and so does the
  saved file, so a printer that adds brackets rewrites the user's formula.

### References & the AST

- **A1 and R1C1**, absolute/relative/mixed (`B7`, `$B$7`, `B$7`), cross-sheet
  (`Sheet2!B7`), 3-D (`Sheet1:Sheet3!A1`), ranges (`A1:C9`), full-row/column
  (`A:A`, `3:3`), structured/table refs (`Table1[Amount]`, Phase 3), and defined
  names.
- **Reference algebra** — the primitives the transaction layer needs to rewrite
  references when rows/columns are inserted or deleted, when cells are moved, and
  when a formula is filled/copied (relative-reference shifting). Living in
  `-formula` (below `-eval`) is what lets Phase 1A editing rewrite references
  without the calc engine.

### AST interning

Formulas are interned in an **AST arena** ([22](22-NORMALIZED-SCHEMA.md)): a
filled-down column of the "same" relative formula shares one AST plus a per-cell
origin, not N copies — essential for the 1M-cell memory budget.

> **Status: evaluator implemented (Phase 2, P2-001).** `casual-calc-eval`
> evaluates the stored formula ASTs by memoized recursive evaluation with
> circular-reference detection, and `recalculate(workbook)` recomputes every
> formula cell's cached value (a correct **full** recalc). Supported:
> arithmetic/comparison/concat/unary operators, cell + range references (same-
> and cross-sheet), defined names, and a starter function library (SUM, AVERAGE,
> MIN, MAX, COUNT, IF, ABS, ROUND). Still to come: the **incremental dependency
> graph** + dirty propagation and the <50 ms budget (P2-002), a broader
> oracle-diffed function library (P2-003), and volatile/spill/iterative calc
> (P2-004).

## `casual-calc-eval` — the engine

### The dependency graph

- Nodes are cells (and, where useful, ranges/spill regions); edges are
  **precedent → dependent**.
- **Range dependencies are bucketed** (block/interval buckets) so a formula over
  `A1:A100000` creates a bounded number of edges, not 100k — a hard requirement
  for the recalc budget ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md) T3).
- The graph is built **incrementally** as formulas are set (from the model's
  reserved `dependency_edges` seam) and maintained on every edit — never rebuilt
  from scratch on each calc.
- **Volatile functions** (`NOW`, `TODAY`, `RAND`, `RANDBETWEEN`, `OFFSET`,
  `INDIRECT`, `CELL`/`INFO` in some modes) are flagged so they recompute every
  calc regardless of dirtiness.
- `INDIRECT`/`OFFSET` (dynamic references) are handled as volatile edges resolved
  at calc time, since their precedents aren't statically known.

### Recalculation

- **Incremental by default:** an edit dirties a cell and its transitive
  dependents (dirty propagation); only the dirty sub-graph recomputes.
- **Ordering:** dependents recompute in dependency order (topological over the
  dirty sub-graph). The imported `calcChain.xml` is treated as a *hint*, not
  truth — the graph is authoritative and the chain is rebuildable
  ([17](17-GLOSSARY.md)).
- **Cycles:** detected; a genuine cycle yields `#REF!`/circular error unless
  **iterative calculation** is enabled in `calc_settings`, in which case it runs
  bounded iterations to a convergence/step cap.
- **Correctness invariant:** the incremental result is **identical to a full
  recalc**, every time. Golden-tested; a Phase 2 exit-gate condition.
- **Full recalc** (on open, or forced) is bounded and cancellable and kept off
  the interactive path.

### The function library

Organized by family, implemented to match Excel semantics closely enough to pass
the oracle:

- Math & trig, statistical, financial, engineering.
- Text, information (`ISBLANK`, `ISERROR`, `TYPE`, …), logical.
- Lookup & reference (`VLOOKUP`, `HLOOKUP`, `XLOOKUP`, `INDEX`/`MATCH`,
  `LOOKUP`, `CHOOSE`, `INDIRECT`, `OFFSET`).
- Date & time (with the 1900/1904 date-system setting honored).
- Dynamic-array/spill functions (`FILTER`, `SORT`, `UNIQUE`, `SEQUENCE`, …).

Each function declares its arity, coercion rules, error propagation, and
volatility. Coercion and **error values** (`#REF!`, `#VALUE!`, `#DIV/0!`,
`#N/A`, `#NAME?`, `#NULL!`, `#NUM!`, `#SPILL!`) follow Excel's rules — this is
where oracle fidelity is won or lost.

### Spill / dynamic arrays

- A formula may return an array that **spills** into a range. The engine tracks
  the spill anchor and region; overlapping content yields `#SPILL!`.
- Spill regions participate in the dependency graph and in layout (the model's
  `CellFlags` spill bits, [22](22-NORMALIZED-SCHEMA.md)).

## Interaction with the rest of the engine

- **Edit → calc:** `casual-calc-transaction` produces a dirty set; the SDK/bridge
  hands it to `-eval`, which recomputes and writes fresh **cached values** back
  into the model.
- **Calc → layout:** layout reads only cached values, so it is oblivious to
  whether calc has run — it just repaints cells whose cached value changed
  (the same `DirtySet` → tile-invalidation path, [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md)).
- **No back-edges:** the model, transaction, layout, and render layers never call
  into `-eval`. Only the host bridges compose it in.

## Fidelity & determinism gates (Phase 2)

- **Oracle diff:** computed values across the formula corpus match LibreOffice
  Calc / Excel.
- **Latency:** worst-case incremental recalc < 50 ms on the WASM baseline.
- **Determinism:** identical results native vs WASM; incremental == full recalc;
  golden recalc results committed.

## Open decisions (to be ADR'd before Phase 2)

- Dependency-graph node granularity (pure cell-level vs cell+range-node hybrid).
- Recalc scheduling (single-pass topological vs generational/chunked for the
  budget).
- Native parallel recalc strategy on Tauri (graph-partition; must preserve
  determinism).
- Precision/rounding policy to match Excel's 15-significant-digit behavior.
