# 19 — Workspace Scaffold & Layer Division

This is the **layer-division contract.** It is design-critical: crate boundaries
and the dependency DAG are fixed *before code* so no layer needs a do-over when
the calc engine (Phase 2) or collaboration (Phase 5) land. Changing a boundary
here requires a new ADR (ADR-003 in [08](08-ADR-REGISTER.md)).

The guiding rule: **a lower layer never knows about a higher one, and the calc
engine is a peer layer that is wired in later — not a field bolted onto the
model.**

## The dependency DAG

```
                          casual-calc-sdk ─────────────┐
                                │                       │
                          casual-calc-wasm ─────────────┤   (host bridges)
        ┌───────────────────────┼───────────────────────┼───────────────┐
        │                       │                       │               │
 casual-calc-render      casual-calc-io          casual-calc-eval   casual-calc-
        │                 │    │    │    │           (Phase 2)      transaction
        │                 │    │    │    │                │              │
 casual-calc-layout   import export ods  │                │         selection
        │    │            │    │    │     │                │              │
        │    │            └────┴────┴─────┘                │              │
        │    │            casual-calc-ooxml                │              │
        │    │                  │                          │              │
        │    │            casual-calc-package              │              │
        │    │                                             │              │
        │    └──────────────┬──────────────────┬──────────┘              │
        │                   │                  │                         │
        │            casual-calc-formula   casual-calc-model ◀───────────┘
        │                   │                  │
        └───────────────────┴──────────────────┘
                     (all read the model / display list)
```

Read it as: **`casual-calc-model` and `casual-calc-formula` are the shared
bedrock.** Everything above depends downward only. `casual-calc-eval` (the calc
engine) sits *beside* transaction/selection and *above* model+formula — it is not
a dependency of the model, so the model exists and is complete without it.

## Crate-by-crate: what it owns, what it must never own

### Bedrock

**`casual-calc-model`** — the normalized workbook. Owns: sheets, the sparse cell
grid, shared strings, styles/number-formats, defined names, merged ranges, sheet
views, IDs, invariants, deterministic snapshot I/O, and the **reserved calc
seams** (formula-AST arena, cached-value slot, dependency-edge side table). It
**depends on `casual-calc-formula`** for the `Expr` type stored in the arena
(per [22](22-NORMALIZED-SCHEMA.md)) — that is the *only* upward-looking edge, and
`casual-calc-formula` has no dependencies, so there is no cycle. Must **not** own:
any evaluation logic, any layout geometry, any XML. It compiles with zero
knowledge of `casual-calc-eval`.

**`casual-calc-formula`** — the formula *language*: tokenizer, parser, AST,
A1/R1C1 reference algebra, pretty-printer, reference rewriting primitives. Owns
the AST type the model stores and the transaction layer rewrites. Must **not**
own: evaluation, the function library, or the dependency graph (those are
`casual-calc-eval`). Splitting parsing from evaluation is deliberate — import
(Phase 1A) needs the parser but not the evaluator.

> **Why formula is its own crate below eval:** the model stores ASTs from
> Phase 1A and the transaction layer rewrites references on row/column insert;
> both need the AST and reference algebra but not the calc engine. If parsing
> lived inside `eval`, Phase 1A would depend on the whole calc engine or the
> model would carry an opaque string it can't rewrite. This split is the single
> most important boundary for "held back, not un-designed."

### Package & import/export (cross-cutting, format-facing)

**`casual-calc-package`** — format-neutral bounded ZIP/OPC substrate: admission,
limits, path safety, part reads. Shared design with OpenDoc's `casual-doc-package`.
Owns limits; owns nothing spreadsheet-specific.

**`casual-calc-ooxml`** — SpreadsheetML OPC reader on top of `-package`:
content-types, relationships, workbook/worksheet discovery, bounded part reads,
an immutable source snapshot. Owns OOXML shape; owns no model mapping.

**`casual-calc-import`** — SpreadsheetML → `model` + compatibility report +
retained source. Depends on `-ooxml`, `-model`, `-formula` (to parse formulas
into the reserved seam). Owns the mapping and the disposition taxonomy.

**`casual-calc-export`** — `model` → `.xlsx`: the byte-identical repackager and
the semantic writer. Depends on `-import` (for retained source) and `-model`.

**`casual-calc-ods`** — `.ods` admission + semantic import/export. Peer of the
OOXML path, and a direct dependency of the SDK exactly as `casual-calc-export`
is — see the amendment below.

**`casual-calc-io`** — format **detection** and the delimited-text adapters.
Delimited text (CSV/TSV/PSV — comma/tab/pipe) is read and written here and does
**not** pass through `casual-calc-package`; only OPC formats (`.xlsx`, `.ods`)
do. `detect` identifies a run of bytes as XLSX, ODS or delimited text without
depending on any format crate, which is what keeps this crate light enough for a
consumer that only wants the CSV reader.

> **Amended by [ADR-022](08-ADR-REGISTER.md).** This crate was described as "the
> adapter registry… the single entry point hosts call to open/save a spreadsheet
> without naming a format". It never was: dispatch grew in `casual-calc-sdk`,
> which is the entry point hosts actually call and the only layer that depends
> on every format crate. Rather than restate the design as whatever the code had
> become — which would have erased a structural decision without anybody making
> it — the registry is split: **detection below, dispatch above**. `-io` gained
> the detection it never had; the SDK keeps the dispatch it grew. Giving `-io`
> dependencies on `-import`/`-export`/`-ods` so the graph matched the original
> sentence was considered and rejected, because it would make a CSV-only
> consumer carry the whole OOXML stack.

### Editing

**`casual-calc-transaction`** — atomic operations and their inverses over the
model: set/clear cells, insert/delete rows & columns (**with reference rewriting
via `-formula`**), merge/unmerge, set styles, set geometry. Every op returns its
inverse. Owns the op set and reference rewriting; owns no calc, no layout.

**`casual-calc-selection`** — active-cell and range selection, validation, and
mapping under edits. Small; depends on `-model` + `-transaction`.

### Calculation (Phase 2, designed now)

**`casual-calc-eval`** — the calc engine: the dependency graph over the model's
reserved edges, incremental recalculation, cycle/iterative handling, volatility,
spill, and the function library. Depends on `-model` + `-formula`. **Nothing
depends on `-eval` except the SDK/wasm bridges** — so the entire stack below
builds, tests, and ships in Phases 0–1E without it. See
[40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md).

### Layout & render

**`casual-calc-layout`** — grid geometry, merged/frozen layout, in-cell text
shaping (`parley`), number-format display, **viewport virtualization**, and the
backend-neutral display list. Reads the model's **cached values** (never calls
the calc engine). Owns all geometry and the display-list contract. See
[42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md).

**`casual-calc-render`** — CPU raster backend executing the display list on
`tiny-skia`, glyphs via `skrifa`. Owns pixels; owns no geometry.

### Host bridges

**`casual-calc-sdk`** — the stable Rust facade: `Engine`, `WorkbookSession`,
commands, events, snapshots. The only crate that composes *everything*, including
`-eval`. Public surface deliberately narrower than internals.

**`casual-calc-wasm`** — the `wasm-bindgen` bridge the browser grid editor drives:
open/render/edit/select/calc/save, hit-testing, viewport queries. Composes the
same layers as the SDK, exposed to JS.

**Two host targets, one core.** OpenCalc runs as a **Tauri desktop app** (native
Rust) *and* a **web app** (WASM) — see [02](02-ARCHITECTURE.md) §Host targets.
The bridge split reflects this:

- **Tauri desktop** consumes `casual-calc-sdk` directly from its Rust command
  handlers; the calc engine runs **native**, not through WASM. An optional thin
  `casual-calc-tauri` crate may hold Tauri-command wrappers / IPC serialization,
  but it is glue — it adds no engine logic and sits above the SDK.
- **Web** consumes `casual-calc-wasm`.

Both bridges compose the *same* core crates, including `casual-calc-eval`. The
engine never calls the platform directly; anything platform-specific enters as
**a value or a predicate the host supplies**, one seam per capability rather than one combined trait (`ADR-019`): the clock is a value, so it cannot change mid-recalculation; cancellation is any `Fn() -> bool`, so a browser closes over `performance.now()` and no engine crate has to name a clock type — `Instant::now` *panics* on wasm32, which is the target that needs cancellation most. There is no threading seam because nothing is parallel yet, and an interface designed against no caller is a guess. Enforced rather than promised: `tools/check-host-seams.py` fails on a `cfg(target_*)` in `crates/`. This is the boundary that keeps native and WASM from forking.

## Boundary invariants (the rules that prevent do-overs)

1. **The model does not depend on the calc engine.** (ADR-005) Layout and render
   read cached values; only the SDK/wasm bridges wire `-eval` in.
2. **Parsing is separate from evaluation.** (`-formula` below `-eval`) Import and
   transactions use the AST without the engine.
3. **Mutation is centralized.** Only `-transaction` mutates the model; every op
   is invertible. Collaboration (Phase 5) rides the same op layer — no model or
   layout change needed.
4. **The display list is the only render input.** (ADR-008) A GPU backend or a
   new target is additive; it never reaches into layout internals.
5. **Virtualization lives in model + layout, not in the host.** (ADR-009) The
   WASM/DOM host draws a display list for a viewport it *asked the layout for*;
   it does not itself decide what to skip.
6. **Format specifics stay in the format crates.** The model, transaction,
   layout, and eval layers are format-agnostic; adding ODS/CSV never touches them.
7. **The engine is host-agnostic; platform access goes through a capability
   trait.** No core crate has a `#[cfg(target_arch = "wasm32")]` fork. Threads,
   clock, and parallelism are host-supplied, so the *same* engine runs native on
   Tauri and as WASM on the web without divergence (ADR to be recorded at
   Phase 0).

## Tooling & fixtures (outside `crates/`)

Mirroring OpenDoc:

- `tools/casual-calc-benchmark` — the benchmark runner (versioned JSON reports).
- `tools/casual-calc-fidelity` — the differential oracle harness vs LibreOffice
  Calc (values and rendered cells).
- `fuzz/` — a separate cargo workspace (pinned nightly) with targets for the
  bounded package reader and the SpreadsheetML/formula parsers.
- `fixtures/` — checksummed corpus (`manifest.json`), synthetic + rights-reviewed
  real-producer `.xlsx`, and a dedicated **formula corpus** for the Phase 2
  oracle.
- `benchmarks/` — committed named-environment baselines (memory, scroll, recalc).

## Workspace-level policy (planned)

- Rust 2024 edition; `unsafe_code = "forbid"`; `resolver = "3"`.
- Pinned toolchain + declared MSRV (set via ADR at Phase 0).
- Release profile: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
- `wasm32-unknown-unknown` in the default target set; `system-fonts` and
  `external-web-fonts` behind features (native-only where relevant), matching
  OpenDoc's font strategy.
