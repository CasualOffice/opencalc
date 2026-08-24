# 44 — Tauri Desktop Shell Design

How OpenCalc ships as a **native desktop application** via Tauri, driving the
engine as **native Rust** — no WASM on the calc hot path. This is the desktop
half of the dual-host design ([02](02-ARCHITECTURE.md) §Host targets); the web
half is `casual-calc-wasm`. Both compose the *same* host-agnostic core.

Status: design note (DOC-014); §"The platform seams" corrected against the code
by `DOC-029`. The desktop shell is host-side glue; it adds no engine logic and
sits above `casual-calc-sdk`. Nothing here is built — `TAURI-001` is
deliberately not started — so read every present tense below as a description of
the *engine* it would sit on, not of a shell that exists.

## Why Tauri, and why native calc

- **Native performance headroom.** The calc engine, layout, and render run as
  native Rust — real threads, no WASM ceiling — so the <50 ms worst-case recalc
  and 60 fps targets ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)) have far more
  margin than the browser. The desktop path may enable **parallel recalc** where
  the dependency graph allows, while producing results **identical** to the WASM
  path (determinism is never traded for parallelism —
  [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)).
- **Small footprint.** Tauri uses the OS webview for the UI shell and a Rust
  backend for logic — no bundled browser engine.
- **One engine, two hosts.** The desktop backend consumes `casual-calc-sdk`
  directly; the web app consumes `casual-calc-wasm`. Nothing in the core forks
  on `#[cfg(target_arch)]` (boundary invariant 7,
  [19](19-WORKSPACE-SCAFFOLD-DESIGN.md)); the one build difference is the
  `shaping` Cargo feature, below.

## Shape

```
┌──────────────────────────── Tauri app ────────────────────────────┐
│  UI shell (OS webview)                                             │
│    grid canvas · toolbars/ribbon · dialogs                        │
│      │  invoke()  ▲  events                                        │
│      ▼            │                                                │
│  Rust backend (Tauri commands)                                    │
│    casual-calc-tauri  (optional thin glue: command wrappers, IPC) │
│      │                                                             │
│      ▼                                                             │
│    casual-calc-sdk  ── WorkbookSession                            │
│      ├── model · transaction · selection                          │
│      ├── casual-calc-eval        (NATIVE calc)                    │
│      ├── casual-calc-layout      (viewport → display list)        │
│      ├── casual-calc-render      (CPU raster; native GPU later)   │
│      ├── casual-calc-import/-export  (xlsx)                       │
│      └── casual-calc-io          (csv/tsv/psv)                    │
└────────────────────────────────────────────────────────────────────┘
```

`casual-calc-io` is the delimited-text adapter and the format registry; it
depends only on `-model` and `-layout`, so `.xlsx` reaches the session through
`casual-calc-import`/`-export` and the format a session was opened as is
`SessionFormat` in the SDK. `.ods` is **not** in that set: `casual-calc-ods`
exists but nothing depends on it, which is `WOPI-07` and stays visible there
rather than being read out of this diagram.

The **UI shell renders a display list**, it is not the source of truth. The
backend owns the workbook; the webview draws what the layout emits and sends
back intents (edits, selections, scroll) — the same engine-authoritative model
as the web host.

## Responsibilities: shell vs engine

| Concern | Owner |
| --- | --- |
| Window, menus, native dialogs, OS integration | Tauri shell |
| File open/save dialogs, filesystem access | Tauri shell; bytes to `casual-calc-sdk` |
| Fonts (system font access) | Tauri shell supplies bytes; engine stays pure |
| Clock and random seed | Shell supplies `SessionConfig::environment` |
| Stopping a long open or recalc | Shell passes a `Cancel` token |
| Threads, parallelism | **Nobody — no seam exists.** See below |
| Workbook model, edits, calc, layout, render | Engine (`casual-calc-sdk`) |
| Determinism, limits, preservation | Engine |

## The platform seams: what exists, and what does not

The engine never calls the platform directly, and that rule holds today — no
crate under `crates/` carries a `#[cfg(target_arch = ...)]` fork, and nothing in
the engine reads a clock or spawns a thread. What carries the rule is **not** a
single host-capability trait: **no such type exists.** There are two narrow
seams, and one capability with no seam at all.

- **Clock and seed — a value, not a trait.** The host supplies the moment
  `NOW`/`TODAY` report and the seed `RAND` draws from as
  `SessionConfig::environment` (`Environment`, `crates/casual-calc-sdk/src/lib.rs:152`),
  installed onto the model as `Workbook::volatile_now` / `volatile_seed`
  (`crates/casual-calc-model/src/workbook.rs:270`, neither serialised). They live
  on the model because an answer has to be reproducible from the workbook alone.
  The web host already drives this: `session_set_clock`
  (`crates/casual-calc-wasm/src/lib.rs:3984`). A Tauri backend does the same
  thing from `SystemTime`; it does not implement an interface to do it.
- **Cancellation — the `Cancel` trait** (`crates/casual-calc-model/src/cancel.rs:32`),
  which any `Fn() -> bool` satisfies. It is what makes the long jobs stoppable:
  `WorkbookSession::open_cancellable` and `WorkbookSession::recalculate_cancellable`
  (`crates/casual-calc-sdk/src/lib.rs:473` and `:955`), delivered by `SEC-012`.
  Deliberately clock-free — `Instant::now` **panics** on
  `wasm32-unknown-unknown` — so a deadline is the host's closure, native or
  browser. Note that this seam is *offered* and not yet *taken*: the web host
  calls the non-cancellable `WorkbookSession::open` and `recalculate`
  (`crates/casual-calc-wasm/src/lib.rs:140` and `:4060`), so no shipped UI can
  actually stop a job. The desktop shell must take it, not re-invent it.
- **Threading / parallelism — absent, and not merely unimplemented.** There is
  no thread pool, no parallel-recalc partitioning, and **no seam for either**;
  `crates/` contains no `std::thread` outside one test. The capability trait
  that [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) boundary invariant 7,
  [02](02-ARCHITECTURE.md) §Host targets and
  [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) all promise is still a
  **pending ADR** ([08](08-ADR-REGISTER.md) §Pending). That promise stands and
  is not withdrawn here — but nothing may be built as though the trait were
  already there.

Native and web cannot silently diverge *today* because the engine has no
`#[cfg(target_arch)]` anywhere in `crates/`, not because a trait forces
agreement. The one place the two builds already differ is
`casual-calc-render`'s `shaping` Cargo feature — on natively, off for
WebAssembly (ADR-018, [64](64-TEXT-SHAPING.md)) — so a native shell renders
Arabic and Hebrew differently from the browser. That divergence is deliberate,
and [64](64-TEXT-SHAPING.md) says a build without shaping *reports* that it
lacks it — but `casual-calc-render`'s public surface exposes nothing to ask, so
the shell cannot read that report today. The moment the desktop path also
introduces threads there is a second difference and still nothing forcing the
two to agree, which is why the pending ADR has to be Accepted before, not
alongside, `TAURI-001`.

## Command surface (illustrative)

Tauri commands wrap `casual-calc-sdk` calls; results/events flow back to the
webview:

- `open_workbook(path) -> WorkbookHandle + CompatibilityReport`
- `save_workbook(handle, path, mode)` (byte-identical vs semantic)
- `viewport(handle, sheet, rect) -> DisplayList` (virtualized window)
- `edit(handle, op) -> Inverse` then `recalc(handle) -> DirtyValues`
- `hit_test(handle, x, y) -> CellAddress`
- `select`, `fill`, `format`, `insert/delete rows/cols`, `undo`/`redo`

Edits, recalc, and viewport queries are exactly the SDK operations the web bridge
also exposes — the command layer is a transport, not logic.

## Rendering on desktop

- Phase 1D: the CPU raster backend (`casual-calc-render`) produces tiles the
  webview presents (e.g. via a canvas/`<img>` bridge or a native surface).
- Later: a **native GPU backend** consumes the same display list (ADR-008) — an
  additive target, no layout change.

## Packaging & security

- Distributed as a signed native bundle per OS (macOS/Windows/Linux), matching
  the Tier-1 platforms.
- The security boundary is unchanged from the engine's:
  ([07](07-QUALITY-SECURITY-AND-COMPATIBILITY.md), [21](21-PARSER-LIMITS.md)):
  no macro execution, no automatic external fetch, bounded admission. The desktop
  shell must not widen it (e.g. it does not auto-open external links from cells).
- Filesystem access is mediated by the shell; the engine receives bytes, never
  raw paths to fetch on its own.

## Open decisions (to ADR before the desktop shell is built)

- ~~**The host capability trait itself.**~~ **Decided: `ADR-019`, Accepted.** The
  single trait was *rejected* in favour of a seam per capability, and that ADR
  names this row as what it unblocks. A clock, a cancellation source and a thread
  pool share only the property that the engine cannot supply them, which is not
  enough to make them one interface — and a combined trait would force the
  browser host to stub the third of it that has no threads. See
  [78](78-HOST-CAPABILITY-SEAMS.md).

  Left listed here as struck through rather than deleted: this section is what a
  reader consults to know whether the shell can be started, and an open decision
  that silently vanishes is indistinguishable from one nobody made.
- Webview ↔ backend transport for large display lists / bitmaps (IPC vs shared
  surface) — must hold the 60 fps budget on desktop.
- Whether `casual-calc-tauri` exists as a crate or the app wires the SDK inline.
- Native GPU backend timing and API (wgpu vs platform-native).
- Parallel-recalc partitioning strategy (shared with [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) open decisions).
