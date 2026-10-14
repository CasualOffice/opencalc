# 100 — Several `<opencalc-sheet>` on one page

**Status: Proposed, 2026-10-15.** Tracker row `UX-SHELL-24`. A design note only:
nothing here is built. It needs an owner decision (§5) before it is.

## 1. The defect

[55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md) §4b promises one editor per
element: "each element imports its own copy of `editor.js` (a distinct URL is
what makes the module instance distinct) and passes a key down so the wasm glue
is instantiated separately too." `embed.js` `#mount` still does exactly that —
`import(new URL(\`editor.js?v=${BUILD}&i=${key}\`, base))` — and it stopped
working when the editor was split into topic modules (`MNT-005`).

`editor.js` is now a one-line `export * from "./editor.core.js"`. A module's own
query string is not inherited by the specifiers it imports, so
`editor.js?…&i=1` and `editor.js?…&i=2` are two module records that both
re-export **one** `editor.core.js`, and every topic module (`editor.paint.js`,
`editor.selection.js`, …) imports that same core. Two elements therefore share:

- `state` (`editor.core.js`, `export const state = {…}`): selection, sheet,
  scroll, edit mode, everything the grid draws from;
- `wasm` / `init` (`export let init, wasm`), rebound by every `start()`;
- `instanceKey`, which `start(key)` overwrites, so the wasm glue *is* imported
  once per element — and the first element's engine is orphaned in memory the
  moment the second one binds;
- `ocRoot`, `ocOverlayHost`, `ocThemeHost` — `setMountRoot` moves every DOM
  lookup to the newest shadow root;
- 141 module-level `let`s in `editor.core.js` (geometry caches, row-band cache,
  drag and touch state, a11y signatures, collaboration timers) and ~34 more
  across the topic modules (`editor.drafts.js` alone has 21);
- 32 `window`/`document` listeners registered at core module scope, plus 8 in
  `editor.dialogs.js` — registered once, so they serve whichever element the
  shared state currently points at;
- `commands.js`'s registry and its single root (`setRoot`), and
  `window.opencalcEditor` (last writer wins).

**Measured** on the installed package (the `SDK-012` fixture, a second element
appended after the first was ready): the second mount does not throw. Worse, it
**silently swaps the first element's workbook**. A1 was set to `from A` through
the first element's API; after the second mounted, the same handle
(`(await a.ready).wasmApi()`) read A1 as empty, `a.ready` and `b.ready` resolve
to modules whose `setMountRoot` is the same function, and `a.save()` would now
save B's workbook. That is a data-loss defect, not only a missing feature, and
§5 asks for a guard regardless of which option is chosen.

## 2. The engine's session model

`crates/casual-calc-wasm/src/lib.rs` keeps **one** session per wasm instance:

```rust
thread_local! {
    static SESSION: RefCell<Option<WorkbookSession>> = const { RefCell::new(None) };
}
pub(crate) fn with_session<R>(f: impl FnOnce(&WorkbookSession) -> R) -> Option<R>
```

Every binding (≈300 `#[wasm_bindgen]` items) reaches the workbook through
`with_session` / `with_session_mut` with no handle argument. Alongside it sit
other per-instance singletons: `COLLAB` (the OT client, `collab.rs`), `CLIP`
(the internal clipboard, `clipboard.rs`), `TIME_BUDGET_MS`, `VOLATILE` and
`HOST_CLOCK` (the host clock and seed).

So **one wasm instance cannot hold two workbooks today.** Two *instances* can:
the glue is a module, importing it under a second URL gives a second
`WebAssembly.Instance` with its own linear memory, and the compiled code is
shared by the browser's module cache. That is what `instanceKey` was for, and it
still works at that layer — the break is entirely on the JavaScript side.

Linear memory per instance starts at the binary's declared minimum, **64 pages =
4 MB**, and grows with the workbook; wasm memory never shrinks, so an instance
keeps its high-water mark until it is dropped. The binary is 8.1 MB; the shipped
JavaScript is 1.6 MB.

## 3. Options

### (a) An instance object over the editor modules

Every module-scope binding becomes a field of a per-element context —
`createEditor({ root, wasm })` returning the object `embed.js` already treats
the module as (`start`, `on`, `runCommand`, `wasmApi`, `save`, …). Topic modules
stop importing `state`, `wasm`, `byId`, `qs` from the core and take the
context instead; the 40 global listeners are attached per instance and removed
on disconnect, routing by which root holds focus or the event target.

- **What changes.** All of the editor: ~11.6k lines of `editor.core.js`, ~12k
  across the topic modules, `commands.js`'s root becoming per instance, and
  `ribbon.js` (already instance-per-root, `ribbon.start({ root, host, editor })`)
  barely at all. It is mechanical but enormous, and every live binding import
  (`import { state, wasm } from "./editor.core.js"`) is a call site.
- **Wasm.** Unchanged: one instance per element, through the glue-per-key that
  already exists. No Rust work.
- **Memory.** One JS copy of the editor for the page; per element, its state,
  its DOM and its wasm instance (≥ 4 MB linear memory plus the workbook).
- **Risk.** High while it is in flight — a refactor touching every editor file,
  where a binding left at module scope is a cross-talk bug that a single-element
  test cannot see. Staged (one topic module at a time onto a context object,
  core last) it is survivable, and each stage is testable with two elements.
- **What it buys.** The end state 55 §4b already names as the right one: one
  realm, direct calls, shared stylesheets (§4a), the cheapest per-element cost,
  and an element that can be disconnected cleanly (today nothing is torn down).

### (b) One editor runtime driving several roots, over a multi-session engine

One engine instance holding N sessions, addressed by handle — either a handle
argument on every binding, or a `session_use(id)` selector the JavaScript calls
before each burst.

- **What changes.** The JavaScript still has to keep per-root selection, scroll,
  geometry, edit state and listeners — that *is* option (a). (b) is (a) plus a
  second refactor in Rust: `SESSION` becomes a map, `COLLAB` per session,
  ~300 bindings take or select a handle, and the `wasm-bindgen` surface the SDK
  types declare (`@opencalc/engine`) changes shape.
- **Wasm.** Does not support it today (§2). A selector is the cheap version and
  the dangerous one: a call made under the wrong current session writes one
  workbook's edit into another, and nothing fails.
- **Memory.** One linear memory for all workbooks: saves ~4 MB minimum per
  element, but memory only grows, so one large workbook's peak is kept for the
  page's life, and every element shares one 4 GB address space and one set of
  security bounds.
- **Risk.** Highest. Shared fate: a trap (panic) in one session poisons the
  instance, taking every element on the page with it. It also gives up the
  per-instance isolation the engine has for free today.

### (c) A same-origin iframe per element, behind the same element API

`<opencalc-sheet>` keeps its API and creates an `<iframe>` pointing at the
packaged `editor.html` (with `?mode=embedded`), instead of a shadow root. Each
frame is its own document, so its own module map: every module, every listener
and the engine are naturally per element — the bug class in §1 cannot occur.

- **What changes.** `embed.js` only, plus a small amount in the page bootstrap
  for theming and chrome. Because the package's assets must already be served
  **from the host's own origin** (the release notes, `opencalc-assets`), the
  frame is same-origin, so the element can call
  `iframe.contentWindow.opencalcEditor` directly — no `postMessage` transport,
  no structured cloning, the same promise-returning methods. 55 §4 already
  weighed this mount; its "second transport" cost largely disappears when the
  frame is same-origin.
- **Wasm.** Unchanged; one instance per frame, one session each.
- **Memory.** The most per element: a whole document and JS realm (the editor's
  1.6 MB of source parsed and its heap per frame), the stylesheets parsed per
  frame instead of adopted once (55 §4a), and the wasm instance. Compiled wasm
  code is usually shared through the browser's code cache, not guaranteed.
- **Risk.** Low for correctness, real for integration: focus moving into and
  out of the frame, the host's global shortcuts versus ours, sizing (the frame
  does not size to content), drag-and-drop of files onto the grid, printing,
  `beforeunload` inside a frame, and hosts with a `frame-src` CSP. The overlay
  problems 55 §4 lists for shadow DOM go away — a frame is its own viewport.

## 4. Comparison

| | (a) instance object | (b) multi-session runtime | (c) same-origin iframe |
| --- | --- | --- | --- |
| Editor JS changed | all of it | all of it | `embed.js` only |
| Rust changed | none | `SESSION`, `COLLAB`, ~300 bindings | none |
| Wasm sessions | 1 per instance (works today) | N per instance (new) | 1 per instance (works today) |
| Per-element memory | state + DOM + ≥ 4 MB | state + DOM + workbook | realm + DOM + CSS + ≥ 4 MB |
| Cross-talk possible | yes, if a binding is missed | yes, plus wrong-session writes | no |
| Shared fate | one realm | one realm and one engine | none |
| Time to ship | weeks, staged | longest | days |

## 5. Recommendation

1. **Now: a guard, whatever is chosen.** A second `start()` against the shared
   core must refuse with a named error ("one editor per page until
   `UX-SHELL-24`; use `mount="frame"`") instead of rebinding the first
   element's engine. Silent workbook swapping is the worst outcome here.
2. **Next: (c), as `mount="frame"`**, the multi-element path, opt-in so a
   single embed keeps the shadow-DOM mount and its direct shared stylesheets.
   It is the only option that is small, and the only one where the defect
   class is impossible rather than merely fixed.
3. **End state: (a)**, staged a module at a time, with a two-element browser
   test as each stage's gate; when it lands the guard lifts and `mount="frame"`
   stays as the isolation option 55 §4 wanted anyway. **(b) is not
   recommended**: it needs all of (a) and adds shared fate and wrong-session
   writes for a saving of a few megabytes per element.

The open decision for the owner: is `mount="frame"` acceptable as the
multi-element answer for the alpha, with (a) scheduled, or should (a) be done
first and multi-element wait for it?
