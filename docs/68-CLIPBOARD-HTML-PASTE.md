# 68 — Design note: pasting from Excel, LibreOffice and Sheets

**For** UX-CLIP-01 (docs/67 wave C). Design first; what is *not* supported is
recorded here as deliberately as what is.

## The defect

Copy-out already writes `text/html` alongside TSV. Paste-in reads
`navigator.clipboard.readText()` and calls `session_paste_tsv`. So formatting is
not lost by accident or by a mapping gap — **it is discarded by construction**,
because the only flavour ever read is plain text. Every spreadsheet a person
copies from puts a styled `text/html` table on the clipboard, and we never look
at it.

## Who parses the HTML

**The browser, in JavaScript.** Two reasons, and the second is the one that
decides it.

Real clipboard HTML is malformed in ways only a real HTML parser survives —
Excel emits conditional comments and `<style>` blocks, LibreOffice emits
`<font>` tags inside `<td>`, Google Sheets emits deeply nested spans. Writing a
tolerant parser is not the interesting part of this work, and getting it subtly
wrong is invisible.

And the alternative puts an HTML parser **into the WebAssembly bundle**, which
[ADR-018](64-TEXT-SHAPING.md) has already been through once: that bundle is
downloaded by every visitor, and it was halved by taking things *out* of it.

The engine therefore receives a parsed, inert description — a grid of cells with
values and named style properties — and applies it. No markup crosses that
boundary in either direction.

## Why parsing it is safe

The whole risk is that clipboard HTML is attacker-controlled: a page can put
anything on the clipboard, and "paste into a spreadsheet" is not a moment anybody
expects to run code.

- `DOMParser.parseFromString(html, "text/html")` yields a document **with no
  browsing context**. Scripts in it never execute. `<img>`, `<iframe>`, `<link>`
  and friends never fetch — the document has no loader to fetch with.
- The parsed nodes are **never inserted** into the live document, and nothing is
  ever passed to `innerHTML`. This is the same class of sink SEC-001 exists to
  remove; this change must not add one.
- Only `textContent` and a fixed allow-list of attributes are read. `href`,
  `src`, `srcset`, `style` `url(...)` values and every `on*` attribute are not
  read at all, so there is nothing to sanitise — a value that is never consulted
  cannot leak.

Stated as a property rather than a promise: **pasting hostile markup must
produce no script execution and no network request.** That is a test, not a
paragraph.

## What is mapped

Per cell, from inline `style` and the presentational attributes real producers
still emit:

| Clipboard | Model |
| --- | --- |
| `font-weight` ≥ 600 / `bold` | `bold` |
| `font-style: italic` | `italic` |
| `text-decoration: underline` | `underline: Single` |
| `text-decoration: line-through` | `strike` |
| `color` | `font_color` |
| `background-color`, `bgcolor` | `fill_color` |
| `font-family` | `font_name` |
| `font-size` | `font_size_hp` (half-points) |
| `text-align`, `align` | `align` (`HAlign`) |
| `vertical-align`, `valign` | `valign` (`VAlign`) |
| `white-space: normal` / `pre-wrap` | `wrap` |
| `mso-number-format` (Excel), `sdnum` (LibreOffice) | `number_format` |
| `rowspan` / `colspan` | a merge |

Colours are normalised to six hex digits; `rgb()` and three-digit forms are
converted, and anything else is dropped rather than guessed.

## Borders

Each cell's own four edges are mapped; `border-collapse` is never consulted.
That distinction is the whole of it. The property decides which of two
*competing* declarations paints a shared edge, and the model has no shared
edges — every cell carries its own four. Applying what a cell declares for
itself is therefore exact rather than approximate, which is what this was
originally deferred over.

A per-edge longhand beats the `border` shorthand, and an explicit
`border-top: none` beats it too — so presence of the declaration is tested, not
whether it parsed to a line.

Widths map by weight: under 1.5px is `thin`, under 2.5px `medium`, above that
`thick`, with `thin`/`medium`/`thick` keywords and `pt` units resolved first.
`double`, `dashed` and `dotted` keep their style instead, since OOXML has those
by name. `groove`, `ridge`, `inset` and `outset` become a solid line of the same
weight: Excel has no such styles, and a line where a line was asked for is
closer than nothing. `none`, `hidden` and a zero width are no edge at all.

The line-style token is validated against the OOXML list on the Rust side of
the boundary before it reaches a `Style`. The parser only ever emits from a
fixed set, so nothing is rejected today; it is checked because the token is
written into the file verbatim, and a value that reaches the writer unchecked
is one a clipboard could choose.

Of the three committed captures only LibreOffice carries a border at all
(`border-bottom:1px solid #000000`). The weight, style and precedence rules
above are therefore covered by a hand-built table as well, since no producer
emits them all.

## What is not mapped, and why

- **Formulas.** No producer puts them in the HTML flavour — Excel, LibreOffice
  and Sheets all emit the *displayed value*. A pasted `=A1+1` would therefore be
  a value that looks like a formula, which is worse than a value.
- **Themes, gradients, rotation, indent.** Not expressible in what the clipboard
  carries.
- **Column widths and row heights.** Excel emits them; applying them on a plain
  paste would silently reshape the sheet somebody pasted *into*, which is not
  what a paste is for, and is not what Excel's own `Ctrl+V` does either. Asking
  for them explicitly is a different question, and has its own answer: Paste
  Special carries widths for a clip copied *within* the editor (`UX-CLIP-02`).
  That path knows the source columns because it captured them on copy; the
  clipboard's HTML does not travel with a clip, so there is nothing here to
  opt into yet.

## How it is verified

Committed clipboard fixtures — real `text/html` captured from Excel, LibreOffice
Calc and Google Sheets, stored as inert `.html` files — pasted through the real
event path, asserting values, merges and every mapped style.

Plus a hostile fixture: `<script>`, `onerror`, an `<img src>` pointing at a URL
that would be observable if fetched. It must paste its *text* and do nothing
else.

And the existing text-only path must not change: a clipboard with no `text/html`
still goes through `session_paste_tsv`, and the internal rich clipboard still
wins when our own copy is unchanged.
