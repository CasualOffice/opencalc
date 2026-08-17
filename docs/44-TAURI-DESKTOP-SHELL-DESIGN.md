# 44 — Tauri Desktop Shell Design

How OpenCalc ships as a **native desktop application** via Tauri, driving the
engine as **native Rust** — no WASM on the calc hot path. This is the desktop
half of the dual-host design ([02](02-ARCHITECTURE.md) §Host targets); the web
half is `casual-calc-wasm`. Both compose the *same* host-agnostic core.

Status: design note (DOC-014). The desktop shell is host-side glue; it adds no
engine logic and sits above `casual-calc-sdk`.

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
  per platform (boundary invariant 7, [19](19-WORKSPACE-SCAFFOLD-DESIGN.md)).

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
│    casual-calc-sdk  ── Engine · WorkbookSession                   │
│      ├── model · transaction · selection                          │
│      ├── casual-calc-eval        (NATIVE calc)                    │
│      ├── casual-calc-layout      (viewport → display list)        │
│      ├── casual-calc-render      (CPU raster; native GPU later)   │
│      └── casual-calc-io          (open/save xlsx/ods/csv)         │
└────────────────────────────────────────────────────────────────────┘
```

The **UI shell renders a display list**, it is not the source of truth. The
backend owns the workbook; the webview draws what the layout emits and sends
back intents (edits, selections, scroll) — the same engine-authoritative model
as the web host.

## Responsibilities: shell vs engine

| Concern | Owner |
| --- | --- |
| Window, menus, native dialogs, OS integration | Tauri shell |
| File open/save dialogs, filesystem access | Tauri shell → `casual-calc-io` |
| Fonts (system font access) | Tauri shell supplies bytes; engine stays pure |
| Threads, clock, parallelism | Shell implements the **host capability trait** |
| Workbook model, edits, calc, layout, render | Engine (`casual-calc-sdk`) |
| Determinism, limits, preservation | Engine |

## The host capability trait (native impl)

The engine never calls the platform directly; it depends on a small
host-capability interface that each bridge implements
([40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md)). The Tauri backend provides
the **native** implementation:

- **Threading / parallelism** — a real thread pool; the calc engine may partition
  an independent dependency sub-graph across threads.
- **Clock** — the system clock for volatile functions (`NOW`, `TODAY`) — supplied
  by the host so evaluation stays a pure function of its inputs.
- **Cancellation** — long jobs (open, full recalc) are cancellable from the UI.

The WASM bridge provides the single-thread/worker + host-clock implementation of
the same trait. Because the trait is the only platform seam, native and web can
never silently diverge.

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

- Webview ↔ backend transport for large display lists / bitmaps (IPC vs shared
  surface) — must hold the 60 fps budget on desktop.
- Whether `casual-calc-tauri` exists as a crate or the app wires the SDK inline.
- Native GPU backend timing and API (wgpu vs platform-native).
- Parallel-recalc partitioning strategy (shared with [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md) open decisions).
