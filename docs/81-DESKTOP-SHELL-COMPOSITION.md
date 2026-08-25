# Desktop shell composition

How the Tauri app is put together, and why there is no crate between it and the
SDK. Decided as `ADR-023`; `TAURI-001` is what it unblocks.

## The question

[44](44-TAURI-DESKTOP-SHELL-DESIGN.md) draws `casual-calc-tauri` as "optional
thin glue: command wrappers, IPC" and lists as an open decision whether it
exists at all, or whether the app wires `casual-calc-sdk` directly.

It has to be settled before the shell is built, because it decides what is
*published*: a crate is a promise to keep an interface, and the cheapest moment
to not make that promise is before anybody depends on it.

## The answer: no crate

**A crate earns its place by having more than one consumer.** `casual-calc-tauri`
would have exactly one — the desktop app — and would sit between two things
that are already interfaces: `casual-calc-sdk`, whose `WorkbookSession` is the
public API by design, and Tauri's own command macros. A wrapper around a wrapper
is not a seam; it is a place for the two sides to drift.

**The glue is already demonstrated to be thin.** `RND-10` moved the browser
editor onto the engine's display list: `editor.paintlist.js` takes a
`DisplayList` and paints it, and the canvas no longer owns any drawing logic of
its own. That is precisely the desktop shell's job as
[44](44-TAURI-DESKTOP-SHELL-DESIGN.md) states it — *the UI shell renders a
display list, it is not the source of truth*. The webview half of the desktop
app is code that now exists and is tested, not code a crate would need to
abstract over.

**The SDK already answers what a shell asks.** Sixty-five public methods,
including `layout(sheet, viewport) -> DisplayList`, which is the whole of the
rendering contract. A Tauri command that forwards a viewport and returns a
display list has nothing left to add.

**And it is the reversible direction.** Extracting a crate later from working
code is mechanical; withdrawing a published one is not. `SDK-009` is the
standing reminder of what a second published surface costs when it is added
before it has to be — the npm packages shipped without type declarations and it
took a row to notice. One public surface, kept good, beats two kept adequately.

## What follows from it

The desktop app is a binary that depends on `casual-calc-sdk` and Tauri, and
nothing else of ours.

**It lives in its own Cargo workspace**, the way `fuzz/` does, because Tauri's
dependency tree is large and every `cargo build --workspace` and
`cargo clippy --workspace` in CI would otherwise carry it.

That is a decision with a known hazard attached, and it is named here rather
than discovered later: `CI-014` was exactly this. `fuzz/` is a separate
workspace, a signature change broke it, every local gate passed, and CI found it
after the merge. A desktop workspace needs the same treatment `check-rust.py`
now gives the fuzz targets — a `cargo check` of its own — on the first day it
exists, not after it has broken once.

## What this does not decide

Three of [44](44-TAURI-DESKTOP-SHELL-DESIGN.md)'s open decisions remain, and
none of them can be answered by reading:

- **Webview ↔ backend transport for large display lists.** `RND-10` measured
  the browser case — about 0.6 ms for a 1434-item frame across the WebAssembly
  boundary, roughly 3.6% of a 60 fps budget. Tauri's IPC is a different
  mechanism and needs its own number, which needs a shell to measure in.
- **Native GPU backend** (wgpu versus platform-native), and its timing.
- **Parallel-recalc partitioning**, shared with
  [40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md).

Each wants a running shell to measure against. Deciding them now would be
guessing, and the guess would be recorded as a decision.
