# ADR-018: Where text shaping belongs, and where it does not

**Status:** accepted
**Relates to** P1C-003, and to ADR-004's per-cell byte ceiling only indirectly.

## The problem as it was stated

The render backend outlines glyphs directly: it maps each `char` to a glyph and
advances by that glyph's width. That is correct for Latin and wrong for anything
needing reordering, ligatures, contextual forms or a bidirectional run — Arabic,
Hebrew, Hindi, Thai. The tracker recorded the fix as "add `parley`", and left it.

## What checking first changed

Two facts, neither of which was in the row:

**The editor does not use this code to draw text.** It draws to a canvas with
`ctx.fillText`, in twenty-two places. The browser shapes that itself, with the
same engine it uses for every other page — so **complex scripts already render
correctly in the editor today**. The defect is not on screen while somebody is
typing.

**`casual-calc-render` is compiled into the WebAssembly bundle**, which is
already 12.9 MB. Adding a shaping stack to that crate adds it to every browser
that loads the editor, including the overwhelming majority who will never render
a PNG of anything.

So the naive reading — "the engine cannot shape text, add a shaper" — would have
taxed every user of the editor to fix a defect none of them can see, in a code
path they mostly do not call.

## Where the defect actually is

The headless PNG backend: `render_pixmap` and everything above it. That is used
for server-side rendering, thumbnails and previews — and it is reachable from
the browser too, through the SDK's `render_sheet_png`. A thumbnail of a sheet
containing Arabic is wrong today, and it is wrong in a way that looks like a
rendering bug rather than an unsupported feature, which is worse.

## The decision

**Shaping goes into `casual-calc-render`, behind a feature that is on by default
and off for WebAssembly.**

- Native builds — the server, the CLI, the fidelity tools — get real shaping.
  That is where PNGs are produced at scale and where correctness for every
  script is not negotiable.
- The WebAssembly build turns it off, and `render_sheet_png` from a browser
  keeps the direct glyph path. The browser is not short of a text shaper; it has
  one, and it is already drawing every cell the user actually looks at.

**The unshaped path is not deleted, and does not pretend.** It stays as the
fallback the feature switches away from, and a build without shaping reports
that it lacks it rather than silently producing wrong output — the same reason
[`Refusal`](../crates/casual-calc-transaction/src/protocol.rs) distinguishes
"not saving" from "read only". A caller rendering a thumbnail can then decide,
rather than discovering it from a customer.

## What was rejected

**Adding `parley` unconditionally.** Simplest, and it puts a font-enumeration
and shaping stack into a 12.9 MB bundle to fix something no editor user sees.

**Shaping in the browser and passing the result down.** Tempting, because the
browser's shaper is right there — and it inverts the dependency: the engine
would need the host to render, so the same document would render differently on
a server than in a tab, and the headless backend would stop being headless.

**Leaving it.** Defensible for an English-only product and not for this one,
which is trying to be an alternative to Excel. A thumbnail that mangles Arabic
is not a missing feature; it is a wrong picture of somebody's document.

## What this does not settle

The 12.9 MB bundle is not addressed here and is a separate question worth
asking. It is noted because it is what made the naive answer expensive, and
because a project targeting a browser should know that number.

## How it will be verified

Rendering a cell of Arabic and one of Devanagari through the native backend and
asserting the glyph run is reordered and joined rather than one glyph per
`char` — which is exactly what the current path produces, so the test fails
before the change and passes after. Plus the existing PNG fidelity tests, which
must not move: shaping Latin must produce what it produces today, or every
existing reference image is wrong.
