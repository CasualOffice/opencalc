# 64 — ADR-018: Where text shaping belongs, and where it does not

**Status: Accepted**
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

## Fonts are ingested, not embedded

The scripts the bundle does not cover — Arabic, Devanagari, Thai, CJK — are
**supplied by the host at runtime** rather than bundled. `register_font(bytes)`
in the browser, `register_face` natively; supplied faces are searched before the
bundled ones, because a host that went to the trouble of providing one did so
precisely because the bundled faces were not enough.

Bundling Noto was the obvious answer and is the wrong shape twice. It puts
megabytes into every tab for scripts most deployments never see — and the
bundle had only just been halved by taking fonts *out*. And it makes this
project the arbiter of which languages are worth carrying, which is not a
judgement it should be making on anybody's behalf. A host knows which scripts
its documents are in, already serves static assets, and can ship one font or
twenty without this crate changing.

What stays embedded is Latin, because something has to work with no
configuration at all.

`register_face` returns whether the bytes were a readable face. The realistic
failure is a host fetching a font URL and getting an error page; storing that
would produce a renderer searching an HTML document for glyphs, and a thumbnail
full of boxes with nothing to explain it.

## Shaping is necessary and not sufficient

Building it surfaced something the plan did not account for: **the bundled fonts
cover Latin and Hebrew, and not Arabic, Devanagari, Thai or CJK.** Caladea,
Carlito, Liberation and Roboto are what ship, and a shaper cannot draw glyphs a
font does not have.

So the scope of this change is narrower and more honest than "complex scripts
now work":

- **Hebrew is fixed.** It is covered and right-to-left, so the per-`char` path
  renders it backwards today. Shaping returns visual order and it comes out
  right. That is a real defect, fixed.
- **Arabic, Devanagari, Thai and CJK are unchanged**, and were already `.notdef`
  boxes rather than mis-shaped text. They need a *font* decision, which is a
  separate and much larger one — Noto's coverage is measured in megabytes, and
  this crate is already in a 12.9 MB bundle.

That distinction is recorded as a test rather than a sentence, so that adding a
font becomes a deliberate act with a size cost attached rather than something
discovered from a screenshot.

## The bundle, since it was noted here

This decision recorded 12.9 MB as a separate question worth asking. Asking it
found the answer in the same place: **9.1 MB of that bundle was embedded fonts**
— 72% of what every visitor downloads to open the editor — and the editor never
reads a byte of them, because it draws text with `ctx.fillText` and the browser
supplies the faces. They exist for the headless PNG backend.

So the same split applies: all six families for native, Carlito alone for
WebAssembly. Carlito is metric-compatible with Calibri, which is what an `.xlsx`
asks for more often than everything else combined, and enough that a thumbnail
has text in it. Anything else falls back to it — visibly the wrong face, which
is an honest failure for a preview and a better trade than a nine-megabyte
download for every visitor who never renders one.

**12.92 MB to 6.53 MB.** Half of it, and the editor is unchanged: the browser
was always drawing its own text.

Doing it also showed why gating the list was not enough. `DEFAULT_FAMILY` still
named Roboto, and a constant naming a blob is what keeps the blob alive — so 2 MB
came back and every substitution still landed on a family the build had
supposedly dropped. It is gated too.

## How it is verified

Not with Arabic, which an earlier draft of this section proposed: there is no
bundled font covering it, so such a test would have asserted the shaping of
`.notdef` boxes and passed for the wrong reason.

**Hebrew**, which *is* covered and *is* right-to-left. The test shapes a
three-letter run and asserts the glyph ids come back as exactly the naive
per-character mapping **reversed** — visual order rather than memory order,
which is the thing the old path cannot produce. Asserted as a relationship
rather than as fixed ids, because the ids belong to whichever face is bundled.

**Coverage itself** is a test: Latin and Hebrew covered, Arabic, Devanagari,
Thai and CJK not. It fails if that changes, so adding a font is a deliberate act
with a size cost rather than something noticed in a screenshot.

**The existing PNG fidelity tests**, unchanged and now running with shaping on.
Latin must render exactly as it did or every reference image is wrong; it does.

**The dependency graph**, checked with `cargo tree`: the shaper is present for
native builds and absent from the WebAssembly one. That is the decision above,
and it is the kind that quietly stops being true — a crate added later with
default features would undo it without a word.

## Amendment: which shaper (`DEP-14`)

The shaper was `rustybuzz`. It and its `ttf-parser` are both unmaintained
(`RUSTSEC-2026-0206`, `RUSTSEC-2026-0192`) — "the author has stopped", not "this
is exploitable" — and for a while there was nowhere to go, so both sat in
`deny.toml` as individually named ignores with reasons.

It is now **`harfrust`**: the same HarfBuzz shaping algorithm, maintained under
the harfbuzz organisation, and built on `read-fonts` — which is what `skrifa`
is built on.

That second part is the reason this is an amendment rather than a version bump.
The argument above for handing shaped glyph *ids* straight to `skrifa` was that
"a glyph id is an index into the font's own tables, so the same bytes give the
same ids to both". That was true, and it was true of **two different parsers**
that happened to agree: `rustybuzz` read the font with `ttf-parser` while
`skrifa` read it with `read-fonts`. Now there is one parser and one set of
tables, and the agreement is structural rather than fortunate. `skrifa` moved
0.43 → 0.46 so that both resolve to a single `read-fonts` in the lock file; two
versions of it would have left the same two-parser argument in place wearing a
different name.

Both `deny.toml` ignores are deleted, which was the acceptance for the row. An
ignore list that empties is one that was being read.
