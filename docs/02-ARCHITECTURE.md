# 02 — Architecture

OpenCalc is a **headless spreadsheet engine with host adapters.** The engine owns
the workbook state, the calculation, the layout, and the display list; the host
owns the window, the fonts, the I/O, and the policy. This doc is the shape; the
crate-level layer division is [19](19-WORKSPACE-SCAFFOLD-DESIGN.md).

## The stack

```
                    ┌─────────────────────────────────────────────┐
   Host App  ─────▶ │  Stable SDK / FFI / WASM bridge              │  casual-calc-sdk
                    │                                             │  casual-calc-wasm
                    ├─────────────────────────────────────────────┤
                    │  Workbook Runtime                           │
                    │   model · transactions · selection         │  casual-calc-model
                    │                                             │  casual-calc-transaction
                    │                                             │  casual-calc-selection
                    ├─────────────────────────────────────────────┤
                    │  Calculation Runtime            (Phase 2)   │  casual-calc-formula
                    │   formula AST · dependency graph · recalc   │  casual-calc-eval
                    ├─────────────────────────────────────────────┤
                    │  Layout Runtime                             │
                    │   grid geometry · virtualization · shaping  │  casual-calc-layout
                    ├─────────────────────────────────────────────┤
                    │  Display List  (backend-neutral)            │  (in casual-calc-layout)
                    ├─────────────────────────────────────────────┤
                    │  Render Backends                            │  casual-calc-render
                    │   CPU raster (tiny-skia + skrifa)           │  (GPU later)
                    └─────────────────────────────────────────────┘

   Cross-cutting:  Import / Export (SpreadsheetML, ODS, CSV, JSON)
                   casual-calc-package · casual-calc-ooxml · casual-calc-import
                   casual-calc-export · casual-calc-io · casual-calc-ods
```

Each layer depends only on those below it. The **calculation runtime is a layer
in the design from day one**, even though it is built in Phase 2 — the model
above it reserves its seams, and the layout below it reads *cached values*, so
neither needs a rewrite when the calc engine lands.

## Core principles

1. **Engine state is authoritative.** The DOM/canvas is a projection of the
   display list, never the source of truth.
2. **Mutation flows through transactions.** Every edit is an operation that
   returns its inverse; undo/redo is inverse replay. No layer mutates the model
   directly.
3. **Calculation is separated from editing.** An edit marks cells dirty; the
   calc engine (Phase 2) recomputes the dirty sub-graph. The model stores a
   *cached value* alongside every formula so layout/render work before the calc
   engine exists and continue to read cached values after.
4. **Layout is virtualized.** The layout runtime answers "what's in this
   viewport" in O(visible), and emits a display list for just that window. See
   [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md), [42](42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md).
5. **Rendering is retained and backend-neutral.** Layout emits a serializable
   display list; backends execute it. This is the golden-testable seam and the
   place a GPU backend plugs in later.
6. **Import preserves intent and unknown data.** Nothing is dropped silently
   ([34](34-SPREADSHEETML-FIDELITY-ARCHITECTURE.md)).
7. **The host controls policy.** Fonts, I/O, network, persistence, collaboration
   transport — all host-supplied. The engine is pure and deterministic.
8. **Public API is narrower than internals.** Internal crates evolve; the SDK
   surface is deliberately small and versioned.

## Host targets: Tauri desktop and web (WASM)

OpenCalc ships to **two first-class hosts over one host-agnostic core.** The core
crates (model, formula, eval, layout, render, import/export) compile unchanged to
native and to `wasm32-unknown-unknown`; only the thin bridge layer differs.

| Host | Bridge | Engine execution | Renderer |
| --- | --- | --- | --- |
| **Tauri desktop app** | `casual-calc-sdk` from Tauri commands (optional thin `casual-calc-tauri` glue) | **Native Rust** — full speed, real threads, parallel recalc where the graph allows | CPU raster now; native GPU later |
| **Web app** | `casual-calc-wasm` (`wasm-bindgen`) | **WASM** in the browser (main thread or Web Worker) | canvas via the display list |
| Headless service | `casual-calc-sdk` | Native Rust | PNG via CPU raster |

Design rules this imposes (enforced from day one):

- **No `#[cfg]` forks in the engine.** Platform-specific needs enter as
  **a value or a predicate the host supplies**, one seam per capability rather than one combined trait (`ADR-019`): the clock is a value, so it cannot change mid-recalculation; cancellation is any `Fn() -> bool`, so a browser closes over `performance.now()` and no engine crate has to name a clock type — `Instant::now` *panics* on wasm32, which is the target that needs cancellation most. There is no threading seam because nothing is parallel yet, and an interface designed against no caller is a guess. Enforced rather than promised: `tools/check-host-seams.py` fails on a `cfg(target_*)` in `crates/`. See [78](78-HOST-CAPABILITY-SEAMS.md).
- **The tight target defines the budget.** The <50 ms recalc and 60 fps budgets
  ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)) are set on the WASM path; the
  Tauri-native path has more headroom but must produce **identical results**.
- **The desktop calc engine is native, not bundled WASM.** On Tauri the formula
  engine is driven by native Rust directly — no WASM boundary on the calc hot
  path — which is why the same engine can be more aggressive (parallel) on
  desktop while staying deterministic across both hosts.

## Independent version numbers

Four things version separately, so one can change without falsely implying the
others changed:

- **SDK API version** — the host-facing surface.
- **Workbook schema version** — the normalized model / snapshot format.
- **Transaction/op schema version** — the edit and (later) collaboration ops.
- **File-compatibility profile** — which SpreadsheetML features are handled.

## Data flow: open → edit → calc → render → save

```
.xlsx bytes
  → bounded OPC admission            (casual-calc-package)
  → SpreadsheetML part/graph read    (casual-calc-ooxml)
  → semantic import + preservation   (casual-calc-import) ─▶ Workbook model
                                                            + compatibility report
                                                            + retained source
  → [edit] transaction               (casual-calc-transaction) ─▶ dirty set
  → [calc] recompute dirty sub-graph (casual-calc-eval, Phase 2) ─▶ cached values
  → layout the viewport              (casual-calc-layout) ─▶ display list
  → render                            (casual-calc-render) ─▶ pixels
  → export                            (casual-calc-export) ─▶ .xlsx bytes
                                        (byte-identical if unedited; semantic if edited)
```

## Security boundary

- No macro / VBA execution, ever.
- No automatic fetching of external references, images, or links.
- All admission is bounded and cancellable ([21](21-PARSER-LIMITS.md)).
- Untrusted input cannot exhaust memory or CPU; the sparse model and virtualized
  layout keep even hostile "1M cells everywhere" inputs within budget or reject
  them cleanly.

## Why this shape survives the phased build

The risk in a phased engine is that a later layer forces an earlier one to be
rewritten. This architecture avoids that by fixing three things now:

- the **model's reserved calc seams** (so Phase 2 adds behavior, not schema),
- the **cached-value contract** (so layout/render never depend on the calc
  engine's existence), and
- the **display-list seam** (so a GPU backend or a new render target is additive).

These are ADR-004/005/008/009 in [08](08-ADR-REGISTER.md).
