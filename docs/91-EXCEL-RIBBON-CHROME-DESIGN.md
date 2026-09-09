# 91 — Excel's ribbon, as this editor's chrome

**Nothing in this note is implemented. No code was changed to write it.** Every
`file:line` below was opened and read; every number about *our* chrome is the
declared value in the stylesheet or a sum of them, and says so. §12 reproduces
it. §10 is the work, sliced; the rows are `UX-RIB-01` … `UX-RIB-13` in
[14](14-EXECUTION-TRACKER.md).

[88](88-EDITOR-CHROME-COMPOSITION.md) §8.1 refused a ribbon. **That refusal is
superseded by the product owner's ruling** — the editor gets Excel's interface —
and `docs/88` is retained here for what it measured: the region heights, the
1463 px width budget and its 1461 px collapse point, the group-boundary budget,
the sheet-strip scroller finding, and the observation that Office's collapse
order is *authored and not computed*. This note does not re-argue the decision.
It specifies the thing.

## Outcome

Afterwards a person who knows Excel can sit down at this editor and use it
without being taught anything, because every command they reach for is on the
tab they expect, in the group they expect, at the size they expect, under the
KeyTip they already type. Concretely, six things become true that are not true
today:

1. **Commands are addressed by name, not hunted for.** Today's inventory of 188
   commands is split across an eight-menu bar and a 35-control toolbar with no
   labels, and ~60 further verbs (Paste Special, Insert Cells with shift,
   AutoFit, Column Width, Convert to Range, every sheet verb) exist **only** in
   context menus, where they carry no `data-oc-command` at all — invisible to
   `listCommands()`, to the desktop OS menu, to a host's `commandRules`, and to
   the read-only whitelist. The ribbon draws them, which moves them from
   *ungoverned* to inside `applyCommandRules()`
   (`webapp/editor.selection.js:1166`). That is the largest single governance
   win in this design and it is free: they go in Excel's own slots.
2. **`Alt` reaches everything.** Alt-H-1 is Bold, Alt-N is Insert, Alt-A-S-A is
   Sort A→Z — the same sequences Excel publishes.
3. **A File backstage exists**, so Properties, Version history, Save a Copy,
   Print and Export stop being scattered between a menu, a gear popover and a
   side panel.
4. **The chrome gets *smaller*, in the default layout.** 144 px above the grid
   against today's 162 (§1). The Classic three-row ribbon is opt-in and costs
   196 px; nobody pays it who does not ask for it.
5. **Collapse becomes a designed sequence rather than an arithmetic accident.**
   Today the first toolbar group folds at a width nobody chose. §5 authors the
   order, names the widths, and asserts them.
6. **Every interactive state is drawn.** There is no `:focus-visible` rule on a
   toolbar button in the product today (`webapp/editor.css:763-772` has rest,
   hover, checked and disabled and nothing else), and `--oc-accent-ring`
   (`webapp/editor.css:118`) is a published token that nothing in the chrome
   uses. §2 spends it.

---

## 0. How this was measured, and what is *not* measured

**Our chrome is exact.** Every height, width, radius and duration attributed to
this product below is the declared value in `webapp/editor.css` or a sum of
declared values, cited to the line. Where a number is a *sum* it shows its
addends. Nothing here was measured in a browser for this note — this is a
design round, no build was run, and §5's width budget is therefore **computed
from §2's metrics, not observed**. §10's acceptance gates are how it becomes a
measurement; until one of them runs, treat the 1390 px figure as arithmetic.

**Excel is estimated, everywhere, without exception.** Microsoft publishes no
device-independent pixel measurement for any chrome region —
[88](88-EDITOR-CHROME-COMPOSITION.md) §0 records the same finding and this note
inherits it. What Microsoft *does* publish, and what this note leans on, is
content: the ribbon tab access keys, the Ribbon Display Options wording, the
Backstage destination list, the Simplified-ribbon behaviour (one row, merged
carets, trailing ellipsis), the Fluent radius and stroke tokens, and the Fluent
type ramp. Every Excel *pixel* in this note is an inference from those and is
marked **[est]** where it is load-bearing.

**Three things are unverified and are marked again where they appear:**

- The **contextual-tab KeyTip letters** (`JT`, `JC`, `JA`, `JD`). Microsoft
  publishes the fixed tabs' access keys and not these. They are Excel's *shape*,
  authored by us.
- **Whether the Simplified layout ships in Excel for Windows desktop.** The
  desktop Ribbon Display Options menu documents three states and does not
  include a Classic/Simplified choice; the *web* menu does. An Insiders-only
  desktop single-row mode existed in 2022 and its own source warned it might be
  dropped. We do not claim desktop parity for the flattened ribbon, and it does
  not matter: the owner asked for the flattened ribbon and the web variant is
  the documented one.
- **The per-command inventory of Excel's own Simplified rows**, and **the widths
  at which Excel's classic groups collapse.** Neither is published anywhere
  findable. §4 and §5 are therefore *ours*, authored in Excel's shape.

**One claim is derived rather than read.** §6's assertion that hiding
`#menubar` with `display:none` leaves `menuModel()` complete is derived from
two facts — `menuModel()` filters on `.oc-cmd-hidden` and not on visibility
(`webapp/editor.selection.js:1131`), and `querySelectorAll` does not consult
layout — plus the empirical fact that the desktop shell has shipped in exactly
that state for months (`webapp/editor.css:2055`). It was not driven in a
browser for this note.

---

## 1. The region stack

Declared heights, mouse metrics, web chrome, header shown. Every figure is
`content + 1px hairline` where the region has one.

### 1.1 Today

| region | height | source |
| --- | --- | --- |
| `.app-header` | 52 + 1 = **53** | `webapp/editor.css:303`, `:308` |
| `.menubar` | 30 + 1 = **31** | `webapp/editor.css:415` |
| `#oc-recovery` | auto, normally **0** | `webapp/editor.css:599` |
| `.toolbar` | 41 + 1 = **42** | `webapp/editor.css:356` |
| `.formula-bar` | 35 + 1 = **36** | `webapp/editor.css:994` |
| **grid** | remainder | sized from `wrap.getBoundingClientRect()`, `webapp/editor.geometry.js:229` |
| `.bottom-bar` | 34 + 1 = **35** | `webapp/editor.css:1037` |

**162 px above the grid, 197 px of chrome in total.** At 1440 × 900 that is a
703 px grid — **78 %** content share. [12](12-COMPETITIVE-ANALYSIS.md) measures
76 % and banks it as an advantage over Sheets; the difference is the header
collapse shipped since.

### 1.2 After

| # | region | Simplified (default) | Classic (opt-in) | what changes |
| --- | --- | --- | --- | --- |
| 1 | Title / QAT strip *(replaces `.app-header`)* | 36 + 1 = **37** | 37 | −16 px. Gains the QAT and the roster; keeps `#doc-name` |
| 2 | Ribbon tab strip *(replaces `.menubar`)* | **30** | 30 | −1 px. No bottom hairline: strip and body are one surface |
| 3 | Ribbon body | 40 + 1 = **41** | 92 + 1 = **93** | Simplified is the current toolbar band to the pixel |
| 4 | `#oc-recovery` | auto, **0** | 0 | Moves one slot down, below the ribbon |
| 5 | `.formula-bar` | 35 + 1 = **36** | 36 | Unchanged |
| 6 | grid | remainder | remainder | Unchanged |
| 7 | sheet tabs + status *(one row)* | 34 + 1 = **35** | 35 | Unchanged |
| | **above the grid** | **144** | **196** | |
| | **total chrome** | **179** | **231** | |

**The Simplified default is 18 px cheaper than the chrome it replaces.** At
1440 × 900: 721 px grid, **80.1 %** — the content-share advantage is enlarged,
not spent. Classic gives 669 px, **74.3 %**, 1.7 points below the number
`docs/12` banks, paid only by a user who asks for it and whose choice persists.
Collapsed-to-tabs (Ctrl+F1) is 103 px above the grid, **84.7 %**.

**Against `docs/88`'s decided 86 px stack** (28 menubar + 32 toolbar + 26
formula bar): that stack has no title strip, so like-for-like is 144 − 37 =
**107 against 86 — the ribbon costs 21 px** in Simplified, 73 px in Classic.

**Against Excel [est]**: ~32-40 title + ~30-32 tabs + ~90-96 classic body + ~24-26
formula + 28 sheet row + ~22-26 status ≈ **226-256 px**. Classic's 231 sits
inside that band; Simplified's 179 is 50-75 px under it.

### 1.3 How the Classic body's 93 px is built

Not copied from Excel — constructed from §2's control metrics:

```
  6 px top padding
+ 3 × 24 px control rows      = 72
+ 14 px group-caption row
= 92 px content + 1 px hairline = 93
```

Excel's classic body is estimated at 90-96 [est]. Ours lands inside that by
construction rather than by coincidence, which is the point of building it from
a control row rather than adopting a figure.

### 1.4 What each region carries

**1. Title / QAT strip.** Left → right: AutoSave pill (only when a collab
session exists); the Quick Access Toolbar — Save (split), Undo (split, undo-stack
menu), Redo — then the Customize caret; a 1 px hairline; then `button#doc-name`
+ `input#doc-rename` + `span#doc-state` exactly as authored today
(`webapp/editor.html:72`). Centre: the Search box (Alt+Q). Right: `#presence`,
**relocated here from `#menubar`**; Share; `button#tb-settings`
(`webapp/editor.html:95`). 36 px = 28 px control + 4 px above and below; 28 is
the pointer-target floor this project already asserts
(`webapp/editor.css:763`).

The strip names the **document**, never the product — `webapp/editor.css:311`
records that deletion and `editor.branding.spec.mjs:57` enforces it. That does
not change.

**2. Ribbon tab strip.** `File` (drawn as a filled tab, not a tab with an
underline — it is a different kind of thing), then Home, Insert, Page Layout,
Formulas, Data, Review, View, Help; contextual tabs append after Help while
their object is selected. Tab caption 13 px / 500, which is exactly what
`.menu-top` is today (`webapp/editor.css:532`), so the strip costs the same ink
the menu bar did. Active tab: a 2 px underline in `--oc-accent-color`, inset
8 px each side. The strip is a horizontal scroller with **◀ ▶ arrows pinned
outside the scroll box on the left** — `docs/88` §5 found exactly this defect in
the sheet-tab strip, where `+` and the all-sheets button scrolled out of reach
at twelve sheets; a tab strip is the same shape and gets the same fix in
advance. Pinned at the right end, outside the scroller: the Ribbon Display
Options caret.

**3. Ribbon body.** §2 for metrics, §3 for contents, §4 for the Simplified row.

**4. `#oc-recovery`.** Unchanged and still `flex: 0 0 auto`, so it takes height
from the grid rather than overlaying it (`webapp/editor.css:594`). It moves from
*between the menu bar and the toolbar* to *between the ribbon body and the
formula bar*, so the ribbon's own height animation never has to account for a
sibling that appears asynchronously.

**5. Formula bar.** Untouched, deliberately. `docs/88` §4's composition already
shipped: one flat bar, one seam, no border on the input, the expand chevron at
the far right (`webapp/editor.css:1009`, `:1667`). It stays `position: relative`
because the syntax-tint mirror is absolutely positioned against it
(`webapp/editor.css:991`). The region between the commands and the data is the
one region Excel and we already agree on.

**7. Sheet tabs + status.** Untouched. One row, not two — Excel and LibreOffice
both spend two rows here (`docs/88` §5), which is 28 px of grid we keep that
they do not. `#tb-status` does not move, does not get renamed, and does not
delay its text: 84 of the 88 browser specs boot on it reading `/^engine v\d/`,
and `editor.chrome-composition.spec.mjs:112` pins it inside `.bottom-bar`.

---

## 2. Control metrics, and every state

### 2.1 Sizes

| class | box | icon | where the number comes from |
| --- | --- | --- | --- |
| Large (icon over label, Classic only) | 68 × 40-84 | 32 px | 6 pad + 32 icon + 4 gap + 26 label (two 12/13 lines) = 68, fitting the 72 px control zone. 32 px is `.tb-icon`'s existing box, `webapp/editor.css:780` |
| Medium (icon + label, Classic) | 24 × ≤148 | 16 px | 24 is the Classic row pitch (3 × 24 = 72). Sibling of the 25 px mouse menu row `UX-CHR-13` chose, `webapp/editor.css:872` |
| Small (icon only, Classic) | 24 × 24 | 16 px | as above |
| Simplified row control | 32 × 32 | 16 px | 32 is the per-button pitch `docs/88` §1.1 measured inside our groups, so the flattened row costs the same per control as the bar it replaces. Clears the 28 px floor |
| Combo — font name | h24 / h28 × **140** | — | |
| Combo — font size | h24 / h28 × **48** | — | |
| Combo — number format | h24 / h28 × **96** | — | `docs/88` §3.3's costed figure for the readout |
| Split, horizontal | primary + **14 px** caret | 16 px | hairline between halves on hover only |
| Split, vertical (large) | 48 top + 20 caret | 32 px | |
| Dialog launcher | 14 × 14 | 14 px | bottom-right of the group's caption row |
| Gallery item, in-ribbon | 44 × 32 | — | |
| Gallery item, drop-gallery | 56 × 40 | — | |

**A large button's label wraps to at most two lines.** A control whose label
needs three lines is a control that should be medium.

**Three radii, no new scale.** 3 px on every ribbon control ≤ 32 px — the value
`.tb-btn` already carries, whose comment argues 8 px is a pill and cites
OnlyOffice's `--border-radius-button-toolbar: 1px`
(`webapp/editor.css:763`). 6 px on gallery items and flyout rows
(`webapp/editor.css:394`). 12 px on menu, gallery and flyout panels
(`webapp/editor.css:851`). **0 on the tab strip, the ribbon body and every
group** — `docs/88` §1.2's finding holds inside a ribbon exactly as it held on a
toolbar: a filled, rounded container behind a group is a segmented-control
idiom, and it is one of the three things the original complaint named.

**Spacing.** Group horizontal padding 8 px (matching `.toolbar`'s
`padding: 0 8px`, `webapp/editor.css:357`). Intra-group control gap 2 px in
Classic, 0 in Simplified — Simplified controls sit flush and the hover fill
provides the separation. Group boundary in Classic: **6 px + 1 px rule + 6 px**,
which is `docs/88` §3.1's decided budget and what `.tb-sep` ships
(`webapp/editor.css:881`). Group boundary in Simplified: **8 px of whitespace,
no rule** — Simplified has no group separators by definition.

**Type.** Tab caption 13/500. Control label 12/400. Large-button label 12/13.
Group caption 12/400 in `--oc-muted-text-color`. Combo value 12.5 px. Menu row
13/500 (`webapp/editor.css:874`). No new family: the stack is the one written
after `all: initial` (`webapp/editor.css:56`), so an embed keeps it.
**`font-variant-numeric: tabular-nums` is mandatory** on the font-size field,
the number-format readout, the zoom readout and `#sel-stats` — already the house
habit at `webapp/editor.css:1076`, `:1119`, `:1952`, `:2141`, `:2298`.

**Icons.** 16 px for small / medium / Simplified, 32 px for large. One stroke
weight across the entire set. Filled variant for a toggle's checked state,
regular for rest — that pairing is what makes *checked* legible without relying
on colour, the same non-colour-signal discipline that gives a coloured sheet tab
a 6 × 6 dot as well as its colour (`webapp/editor.css:1051`).

### 2.2 Tokens

Three are new and internal. Adding a token to `sdk/theme-tokens.json` is a
deliberate act — the gate is one-directional, the stylesheet may hold tokens the
manifest does not advertise — and these do not go in it.

```
--oc-pressed-color:        var(--oc-surface-color)
--oc-ribbon-tab-underline: var(--oc-accent-color)
--oc-contextual-tab-tint:  color-mix(in srgb, var(--oc-accent-color) 12%, var(--oc-background-color))
```

`--oc-pressed-color` fills a real gap: there is **no `:active` rule on
`.tb-btn` at all** today. Every one of the three must be declared in all three
token blocks — `:root, :host` (`webapp/editor.css:70`),
`:root[data-theme="dark"]` (`:128`) and the `prefers-color-scheme: dark` block
guarded `:not([data-theme="light"])` (`:172`) — or the manual choice or the OS
one breaks.

### 2.3 Every state, per control kind

**Icon button (small / medium / Simplified).** The house model, extended:

| state | background | glyph | elevation | outline |
| --- | --- | --- | --- | --- |
| rest | transparent | `--oc-icon-color` | none | none |
| hover | `--oc-background-color` | `--oc-text-color` | `--oc-control-shadow` | none |
| **pressed** | `--oc-pressed-color` | `--oc-text-color` | **none** | none |
| **focus-visible** | as underlying state | as state | as state | `2px solid var(--oc-accent-color)`, offset 1 px, **plus** `0 0 0 4px var(--oc-accent-ring)` |
| checked | `--oc-background-color` | `--oc-accent-color` | `--oc-control-shadow` | none |
| checked + hover | `--oc-accent-soft` | `--oc-accent-color` | `--oc-control-shadow` | none |
| disabled | transparent | `--oc-disabled-color` | none | `cursor: default` |

Rest, hover, checked and disabled are exactly what ships
(`webapp/editor.css:763-772`): the control **lifts** off the bar rather than
tinting. Pressed inverts that — the shadow drops and the fill recesses, so a
press reads as a press. Focus-visible is where `--oc-accent-ring` finally gets a
user.

**Large button.** The same table; the fill and ring wrap the whole 68 px box.
Its label does not change colour between rest and hover — a two-line label that
shifts colour under the cursor reads as a link.

**Toggle.** The same table, and `checked` additionally swaps the icon to its
Filled variant so the state survives greyscale and `prefers-contrast: more`.
`aria-pressed` carries it.

**Split button.** Two real `<button>`s in a `role="group"`, never one button
with two hit zones.

| | primary half | caret half |
| --- | --- | --- |
| hover on primary | fill + shadow, and a 1 px `--oc-border-color` hairline appears between the halves | unchanged |
| hover on caret | unchanged | fill + shadow, same hairline |
| pressed | `--oc-pressed-color`, that half only | that half only |
| menu open | rest | `--oc-surface-color`, `aria-expanded="true"`, caret rotates 180° |
| checked (latched primary) | fill + `--oc-accent-color` glyph | unchanged |
| focus-visible | ring on the focused half, `outline-offset: -1px` so the two rings never touch | as left |
| disabled | **both halves together** — a caret opening a menu of dead commands is worse than no caret | as left |

**Combo box.** Rest: transparent field, 1 px `--oc-control-border-color` bottom
edge only. Hover: full 1 px `--oc-border-hover-color` border. Focus-visible:
2 px accent, `outline-offset: -1px` — **inset**, so the field does not grow; a
combo that grows on focus reflows its neighbours. Open: `aria-expanded`, caret
rotated. Invalid (a font name that does not exist, a bad custom format): 1 px
`--oc-danger-color` + `0 0 0 3px var(--oc-danger-ring)` — the second published
token nothing uses today. Disabled: `--oc-disabled-color` text.

**Menu / gallery row.** Rest transparent; hover `--oc-surface-color`, **flat, no
lift**. The house split is deliberate and is kept: *a pill on a bar lifts, a row
in a panel tints* (`webapp/editor.css:877` against `:767`). Selected: 
`--oc-accent-soft` plus a ✓ in `.mi-check`. Focus-visible: 2 px accent inset.
Disabled: `--oc-disabled-color`, `pointer-events: none`, **and the `disabled`
attribute** — `.oc-cmd-disabled`'s `opacity: .45` (`webapp/editor.css:300`) dims
a control a screen reader still announces as actionable.

**Gallery item.** Rest 1 px `--oc-border-color`; hover 2 px accent outline at
offset 1 px (the pattern `.swatch-menu button:hover` already uses,
`webapp/editor.css:930`); selected `aria-selected` plus a `--oc-text-color`
border — a non-colour signal, because a colour swatch's own colour would swallow
a coloured selection mark.

**Ribbon tab.** Rest `--oc-muted-text-color` on transparent. Hover
`--oc-text-color` on `--oc-surface-color`. Selected `--oc-text-color` at 600
plus the 2 px underline. Focus-visible 2 px accent at `outline-offset: -2px` —
inset, because an outset ring on a tab collides with its neighbour. Contextual:
caption in `--oc-accent-color`, strip segment tinted
`--oc-contextual-tab-tint`. **A tab is never disabled**; a tab with nothing in
it does not exist.

**Loading, empty, error — the three a ribbon actually has.** *Loading*: only two
controls can be pending (a gallery whose thumbnails render async, and Version
History). Both show a 16 px skeleton at `--oc-surface-color` **in the item's
exact final box**, so nothing reflows when content lands. *Empty*: an empty
gallery shows one labelled row — "No custom styles yet · New Cell Style…" —
never an empty panel. *Error*: a command that throws puts its message in
`#tb-status`, the one place the product already reports errors
(`webapp/editor.html:597`); the control returns to rest and never latches.

### 2.4 Motion

Motion today is five `transition:` declarations and one `@keyframes` in 2 299
lines (`webapp/editor.css:429`, `:526`, `:765`, `:967`, `:1394`). The global
reduced-motion clamp at `webapp/editor.css:1503` already forces every duration
to 0.01 ms, so everything below inherits the guard for free.

```
--oc-dur-0:  0ms     reduced motion, and press-in
--oc-dur-1:  100ms   hover, press-out
--oc-dur-2:  140ms   caret rotation, underline width
--oc-dur-3:  160ms   menu / gallery / picker open
--oc-dur-4:  220ms   ribbon height change, Classic⇄Simplified
--oc-dur-5:  280ms   backstage entry
--oc-ease-standard: cubic-bezier(0.4, 0, 0.2, 1)
--oc-ease-out:      cubic-bezier(0.16, 1, 0.3, 1)
--oc-ease-in:       cubic-bezier(0.7, 0, 0.84, 0)
--oc-ease-emphasis: cubic-bezier(0.2, 0, 0, 1)
```

`--oc-dur-1` is not invented: it is literally `.tb-btn`'s shipped
`transition: background .1s, color .1s, box-shadow .1s`
(`webapp/editor.css:765`). `--oc-dur-2` sits beside the caret's shipped
`transform 120ms ease` (`webapp/editor.css:429`).

**Tab switch.** Outgoing panel opacity 1→0 over 60 ms `ease-in`; incoming
opacity 0→1 and `translateY(-4px)→0` over 140 ms `ease-out` starting at 40 ms —
an 80 ms overlap, so the ribbon is never blank. Total 180 ms. The active-tab
underline is **one** 2 px element that *travels* (`translateX` + `scaleX`,
200 ms `ease-emphasis`), not two underlines cross-fading, because a travelling
mark says which way you moved. **The body's height does not change between
tabs** — that is a contract, not an outcome — so a tab switch animates opacity
and a 4 px transform only, both compositor-only, and `resize()` is never called.

**Ribbon expand / collapse, and Classic ⇄ Simplified.** Height over 220 ms
`ease-emphasis`. **During the animation the body is `position: absolute` over
the grid with `will-change: transform`, so no reflow occurs; on `transitionend`
it returns to flow and `resize()` runs exactly once.** This is the single most
important rule in the section: `resize()` reads
`wrap.getBoundingClientRect()` and reallocates the canvas
(`webapp/editor.geometry.js:229`), and running it per frame for thirteen frames
is precisely the grid contention this product cannot afford. Controls do not
tween their own size — a 24→32 px tween on forty controls is forty
layout-affecting animations.

**Menus, split carets, galleries, colour pickers.** Open: opacity 0→1 over
120 ms, `scale(0.96)→1` over 160 ms `ease-out`, `transform-origin` at the corner
nearest the trigger. Close: opacity→0 over 90 ms `ease-in`, **no scale** — a
panel that shrinks as it leaves reads slower than one that simply goes. Gallery
items stagger 12 ms each, capped at eight (96 ms); items nine onward arrive with
the eighth, because past ~100 ms a stagger stops reading as choreography and
starts reading as lag.

**Hover and press.** Hover 100 ms. Press **0 ms in, 100 ms out** — a press must
be instantaneous and the release elastic; a 100 ms press-in makes a fast
clicker's feedback arrive after their finger has left. Nothing scales on press:
0.97 on a 24 px control is 0.7 px of travel, invisible, and it costs a
compositor layer per control.

**The focus ring is not animated at all.** A ring that fades in is a ring that
is absent for the first frame after a keypress, and keyboard users arrive at
speed.

**Contextual tabs.** Appear: width 0→W over 160 ms `ease-out`, label fading in
over the final 80 ms. Disappear: 120 ms `ease-in`. If the contextual tab was
active when its object was deselected, the strip's width change completes
**first**, then the tab switch runs — never overlapping, or the underline
travels toward a target that is still moving. **Gate the appearance on the
selection settling (120 ms), not on every selection change**: during a
drag-select the object under the cursor changes every mousemove, and a tab
flickering at 60 Hz is both ugly and a guaranteed frame-budget fight.

**Deliberately not animated, and why.**

- **Nothing in the chrome animates while the grid is painting.** A root class
  applies `transition: none` to the whole ribbon while a frame is pending. The
  product targets 60 fps over a million cells
  ([30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)); a chrome transition stealing
  3 ms of a scroll frame is a regression in the thing this product is good at.
- **No `backdrop-filter` anywhere in the ribbon.** It forces a full-surface
  readback per frame, and `docs/88` §1.2 already deleted the one instance we had.
- **No simultaneous `box-shadow` transitions.** Shadow is paint-bound. One
  control at a time is what ships and is fine; forty is not. The Show-tabs-only
  fly-out ribbon carries a **static** shadow on a transform-animated layer.
- **No JS timers in the chrome.** The marching-ants crawl is the only JS
  animation in this product, and it is stopped in JS for reduced motion
  precisely because it is a timer rather than a transition
  (`webapp/editor.css:1501`). The ribbon adds none.
- **Scroll does not auto-collapse the ribbon.** Excel does not do it, and tying
  a chrome height change to a scroll event is the worst possible coupling to the
  grid's critical path.

**Under `prefers-reduced-motion: reduce`:** backstage, ribbon expand/collapse,
layout switch, contextual appearance and panel scale become instant; stagger →
0. Hover, press and checked are kept, because the clamp already makes them
instantaneous colour swaps and an instant colour swap is correct. The tab
underline's `transform` needs a **targeted** `transition: none` beyond the
clamp — a 0.01 ms transform is still a transform — using the same
targeted-override pattern as `webapp/editor.css:431` and `:527`. **The hard
rule: the reduced-motion end state must be pixel-identical to the animated
one.** A reduced-motion path that also changes layout is a second design nobody
reviewed, and the gate compares settled bounding boxes rather than asserting
that durations are zero.

---

## 3. The assignment

This section is the bulk of the note, and it is the bulk of the work. It assigns
every one of the 188 commands this build has to a tab, a group, a position and a
size, in Excel's taxonomy.

### 3.0 Six rules the assignment follows

**R1 — every command keeps exactly one owning DOM node, and every tab is in the
DOM at all times.** `runCommand(id)` dispatches by `node.click()`
(`webapp/editor.selection.js:1146`) and `listCommands()` reads
`[data-oc-command]` off the live DOM filtered on `.oc-cmd-hidden`
(`webapp/editor.selection.js:1040`). A ribbon that renders only the active tab
silently deletes commands from the SDK, from `menuModel()` and from the desktop
OS menu the moment the user switches tabs. All tabs render; inactive panels hide
with CSS. **This is a design decision, not an implementation detail**, and it is
the one that most constrains the build.

**R2 — where Excel draws one verb twice, the second slot is a proxy.** A proxy
carries no `data-oc-command`; it carries `data-oc-proxy="<id>"` and clicks the
owning node — the pattern `MENUS` already uses fourteen times
(`webapp/editor.core.js:9638`, `:9742`, `:9819`, `:9887`). Proxy ids in the
tables are suffixed `.proxy` and mint nothing. Gridlines, Headings, Filter,
Sort, Protect Sheet, Lock Cell, Refresh All, Version History and the
collaborator roster each appear twice this way.

**R3 — the eight menus survive as a hidden tree, and that is where the
menu-derived half of each merged pair lives.** §6 decides this. Eleven ids are a
second name for a verb the ribbon draws under its toolbar id —
`format.bold/italic/underline/strikethrough`, `edit.undo`, `edit.redo`,
`file.open`, `format.number.currency`, `data.filter`, plus the two menu↔menu
duplicates `view.settings`↔`tools.settings` and
`insert.pivottable`↔`data.pivottable-fields`. They are drawn in the tables as
*(menu-tree alias)* so the count is visible; they render `hidden` in the ribbon
and live in the menu tree.

**R4 — Excel's names, Excel's order, Excel's sizes, even where ours read
better.** Merge & Center is a split at the right end of Alignment. Wrap Text is
a split whose default click wraps. Insert / Delete / Format are three large
buttons in Cells, in that order. Sort & Filter and Find & Select are the two
large menus that close Editing. Where our label is clearer — "Cell markings" for
the A/B/C strips — the ribbon uses Excel's word ("Headings"), because the user
this round is for is the one arriving from Excel; our better word survives in the
tooltip.

**R5 — category errors are refused, even when they would fill a gap.** Three
were declined and each is named in §8.

**R6 — collapse order is authored per tab, lowest number first.** Every group
carries a `collapseOrder`. Nothing is measured; §5's collapse *point* is
measured, the *order* never is.

### 3.1 Reading the tables

`size` is `L` large / `M` medium / `S` small. `kind` is button, toggle, split,
menu, combo, gallery, checkbox, spinner, label. **`status`** is `live` — the
command exists in this build and the ribbon draws it — or **`gap`** — Excel has
it, we do not, and the control is *not drawn* (§8). Gaps are listed so the
inventory is countable, not so they are built.

### 3.2 File — the Backstage rail

Not a ribbon tab. Its destinations are modelled as groups so every `file.*` id
is placed and countable; §6 specifies the surface.

| group | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- |
| Home | Recent workbooks | `file.backstage.recent` | L | gallery | gap |
| Home | Unsaved drafts | `file.backstage.drafts` | L | gallery | live (`webapp/editor.drafts.js:209`) |
| New | Blank workbook | `file.new` | L | button | live |
| New | Template gallery | `file.backstage.templates` | L | gallery | gap |
| Open | Browse… | `file.open` | L | button | live |
| Open | (hidden picker node) | `toolbar.open` | S | button | live |
| Open | Recent / Shared with Me / Add a Place | `file.backstage.open-*` | L | gallery | gap |
| Info | Properties | `file.properties` | L | button | live |
| Info | Version History | `file.version-history` | L | button | live |
| Info | Protect Sheet | `format.protection.protect-this-sheet` (proxy) | L | button | live |
| Info | Check Compatibility | `file.check-compatibility` | L | button | gap — see §8 |
| Info | Inspect Workbook / Browser View Options | `file.inspect-*` | L | menu | gap |
| Save | Save | `file.save` | L | button | live, desktop only (`CHROME_ONLY.native`, `webapp/editor.core.js:1377`) |
| Save a Copy | Save a Copy / Download | `file.download` | L | menu | live |
| Save a Copy | Same format as opened | `file.download.same-format-as-opened` | M | button | live |
| Save a Copy | Excel Workbook (.xlsx) | `file.download.excel-xlsx` | M | button | live, **generated** |
| Save a Copy | Excel Macro-Enabled (.xlsm) | `file.download.excel-macro-enabled-xlsm` | M | button | live, generated |
| Save a Copy | OpenDocument (.ods) | `file.download.opendocument-ods` | M | button | live, generated |
| Save a Copy | CSV (.csv) | `file.download.csv-csv` | M | button | live, generated |
| Save a Copy | Tab-separated (.tsv) | `file.download.tab-separated-tsv` | M | button | live, generated |
| Save a Copy | Pipe-separated (.psv) | `file.download.pipe-separated-psv` | M | button | live, generated |
| Save a Copy | Excel Binary (.xlsb) | — | M | button | gap |
| History | Version history | `file.version-history` (proxy) | L | button | live |
| Print | Print | `file.print` | L | button | live |
| Print | Page Setup | `file.page-setup` | M | button | live |
| Print | Breaks | `file.page-break-here` | M | button | live |
| Print | Live preview, Copies, Printer, Print-what | `file.print.*` | M | — | gap |
| Share | Share with People | `file.share` | L | button | live |
| Share | Collaborators | `#presence` (proxy) | L | menu | live |
| Share | Email / Present Online | `file.share.*` | L | menu | gap |
| Export | Create PDF Document | `file.export-as-pdf` | L | button | live |
| Export | Change File Type | the generated set again | L | gallery | live |
| Publish | Publish to… | — | L | button | gap — a capability seam, §8 |
| Close | Close | — | L | button | gap — §8 |
| Account | Office Theme — Auto | `view.theme.auto` | S | toggle | live |
| Account | Office Theme — White | `view.theme.light` | S | toggle | live |
| Account | Office Theme — Black | `view.theme.dark` | S | toggle | live |
| Account | Accent | `#set-accent` | M | gallery | live |
| Account | Scroll speed | `#set-scroll` | M | spinner | live |
| Account | Language | `review.language` (proxy) | M | combo | live, host-gated |
| Account | About | `help.about-*` | L | button | live — **id is brand-dependent**, §8 |
| Feedback | Feedback | — | L | menu | gap |
| Options | Options | `view.settings` | L | button | live |
| Options | (menu-tree alias) | `tools.settings` | S | button | live |
| Options | (header gear alias) | `toolbar.settings` | S | button | live |
| Options | Customize Ribbon / Quick Access Toolbar / Trust Center | — | M | button | gap |

**The theme controls MOVE here from View, they are not copied.** Excel's only
appearance control is File ▸ Account ▸ Office Theme and it has none on View.
The repo rule that there is exactly **one** theme control
(`webapp/editor.html:132`, asserted by `editor.native-chrome.spec.mjs:646` and
`:677`) is kept — the control moves. `editor.native-chrome.spec.mjs:787`, which
finds the theme by looking for a menu-bar button labelled "View", must be
re-pointed at the backstage route **in the same change**. That is a decision
retaken in §9, not a test relaxed.

**The seven download rows are generated, never hand-authored.**
`downloadItems()` (`webapp/editor.sheets.js:619`) over `writableFormats()`
(`:586`) over `writable_extensions()`
(`crates/casual-calc-wasm/src/io.rs:267`). The ribbon layout calls the
generator; a format the engine learns appears without anyone editing the ribbon.

### 3.3 Home

The widest tab, by construction. Collapse order is the rightmost column;
1 collapses first.

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Clipboard | 6 | Paste | `edit.paste` | L | split | live |
| Clipboard | | Paste Special… | `edit.paste-special` | S | button | live (context-menu only today) |
| Clipboard | | Values / Formulas / Formats / Transpose | `edit.paste-values` `.paste-formulas` `.paste-formats` `.paste-transpose` | S | button | live (context-menu only today) |
| Clipboard | | Cut | `edit.cut` | M | button | live |
| Clipboard | | Copy | `edit.copy` | M | split | live |
| Clipboard | | Copy as Picture | — | S | button | gap |
| Clipboard | | Format Painter | `toolbar.painter` | M | toggle | live |
| Clipboard | | *launcher* → Office Clipboard pane | — | | | gap |
| Font | 9 | Font | `toolbar.font` | M | combo | live |
| Font | | Font Size | `toolbar.size` | M | combo | live |
| Font | | Increase / Decrease Font Size | `toolbar.size-up` `toolbar.size-down` | S | button | live |
| Font | | Bold | `toolbar.bold` (+ `format.bold` alias) | S | toggle | live |
| Font | | Italic | `toolbar.italic` (+ `format.italic` alias) | S | toggle | live |
| Font | | Underline | `toolbar.underline` (+ `format.underline` alias) | S | split | live |
| Font | | Double Underline | — | S | button | gap |
| Font | | Strikethrough | `toolbar.strike` (+ `format.strikethrough` alias) | S | toggle | live |
| Font | | Superscript / Subscript | `format.superscript` `format.subscript` | S | toggle | live |
| Font | | Borders | `toolbar.border` | S | split | live |
| Font | | ↳ 17 placements, 6 line styles, 8 colours | generated by `buildBorderMenu` (`webapp/editor.core.js:5766`) | S | gallery | live |
| Font | | Draw Borders / More Borders… | — | S | menu | gap |
| Font | | Fill Color | `toolbar.fillcolor` | S | split | live |
| Font | | Font Color | `toolbar.fontcolor` | S | split | live |
| Font | | *launcher* → Format Cells ▸ Font (Ctrl+1) | `format.format-cells` | | | live |
| Alignment | 8 | Top / Middle / Bottom Align | `format.alignment.top` `.middle` `.bottom` | S | toggle | live |
| Alignment | | Justify / Distributed (vertical) | `format.alignment.justify-vertical` `.distributed-vertical` | S | button | live |
| Alignment | | Orientation | `toolbar.rotate` → 6 angles | S | menu | live |
| Alignment | | Text Direction (LTR/RTL) | — | S | menu | gap |
| Alignment | | Wrap Text | `toolbar.wrap` → `format.text-overflow.wrap` / `.overflow` / `.clip` | S | split | live |
| Alignment | | Align Left / Center / Right | `format.alignment.left` `.center` `.right` | S | toggle | live |
| Alignment | | Fill, Justify, Center Across Selection, Distributed, Clear | `format.alignment.fill-repeat-text` `.justify` `.center-across-selection` `.distributed` `.clear-general` | S | button | live |
| Alignment | | Decrease / Increase Indent | `toolbar.indent-less` `toolbar.indent-more` | S | button | live |
| Alignment | | Merge & Center | `toolbar.merge` → `.center` `.across` `.all` `.none` | S | split | live |
| Alignment | | *launcher* → Format Cells ▸ Alignment | `format.format-cells` | | | live |
| Number | 7 | Number Format | `toolbar.numfmt` (readout `toolbar.numfmt-label`) | M | combo | live |
| Number | | ↳ the **unified 15-entry list** | see note below | S | toggle | live |
| Number | | Accounting Number Format | `toolbar.currency` | S | split | live — **mislabelled**, §8 |
| Number | | Percent Style | `toolbar.percent` | S | button | live |
| Number | | Comma Style | `toolbar.comma` | S | button | live |
| Number | | Increase / Decrease Decimal | `toolbar.inc-dec` `toolbar.dec-dec` | S | button | live |
| Number | | Fraction | — | S | toggle | gap |
| Number | | *launcher* → Format Cells ▸ Number | `format.format-cells` | | | live |
| Styles | 5 | Conditional Formatting | `format.conditional-formatting` | L | menu | live |
| Styles | | ↳ Manage Rules… | `format.conditional-formatting-rules` | S | button | live |
| Styles | | ↳ Highlight Cells, Top/Bottom, Data Bars, Color Scales, Icon Sets, Clear Rules | — | S | menu | gap |
| Styles | | Format as Table | `home.styles.format-as-table` | L | gallery | live (creates; no style set — §8) |
| Styles | | Cell Styles | `format.cell-styles` | L | gallery | live |
| Styles | | New Cell Style… / Merge Styles… | — | S | button | gap |
| Cells | 4 | Insert | `insert.rows-above` | L | split | live |
| Cells | | ↳ Insert Cells… (shift) | `insert.cells` | S | button | live (context-menu only today) |
| Cells | | ↳ Rows Below / Columns Left / Columns Right / Sheet | `insert.rows-below` `.columns-left` `.columns-right` `insert.sheet` | S | button | live |
| Cells | | Delete | `insert.delete-rows` | L | split | live |
| Cells | | ↳ Delete Cells… (shift) / Columns / Sheet | `insert.delete-cells` `.delete-columns` `toolbar.delete-sheet` | S | button | live |
| Cells | | Format | `home.cells.format` | L | menu | live |
| Cells | | ↳ Row Height… / Column Width… | `format.row-height` `format.column-width` | S | button | live (header context menu only today) |
| Cells | | ↳ AutoFit Row Height / Column Width | `format.autofit-row` `format.autofit-column` | S | button | live (header context menu only today) |
| Cells | | ↳ Hide Rows / Hide Columns | `data.hide-rows` `data.hide-columns` | S | button | live |
| Cells | | ↳ Unhide in selection / Unhide all | `data.unhide-rows-columns-in-selection` `data.unhide-all-rows-and-columns` | S | button | live |
| Cells | | ↳ Organize Sheets: Rename, Duplicate, Tab Color, Hide, Unhide *(generated)* | `sheet.rename` `sheet.duplicate` `sheet.tab-color` `sheet.hide` `sheet.unhide` | S | button | live (tab context menu only today) |
| Cells | | ↳ Move or Copy Sheet… / Default Width… | — | S | button | gap |
| Cells | | ↳ Protect Sheet / Lock Cell | proxies of `format.protection.*` | S | toggle | live |
| Cells | | ↳ Format Cells… | `format.format-cells` | S | button | live |
| Editing | 3 | AutoSum | `formulas.autosum` | M | split | **gap-with-a-verb** — `autoSum()` exists (`webapp/editor.core.js:8282`) with no DOM node; this row mints its first pointer route |
| Editing | | ↳ Average / Count / Max / Min | — | S | button | gap |
| Editing | | ↳ More Functions… | `formulas.insert-function` (proxy) | S | button | live |
| Editing | | Fill | `edit.fill` | M | menu | live |
| Editing | | ↳ Down / Right | `edit.fill.fill-down` `.fill-right` | S | button | live |
| Editing | | ↳ Series / Growth / Copy cells / Formatting only / Without formatting | `edit.fill.fill-series` `.growth-series` `.copy-cells` `.formatting-only` `.without-formatting` | S | button | live |
| Editing | | ↳ Up / Left / Across Worksheets / Justify / Flash Fill | — | S | button | gap |
| Editing | | Clear | `edit.clear` | M | menu | live |
| Editing | | ↳ All / Formats / Contents | `edit.clear.all` `.formatting` `.values` | S | button | live |
| Editing | | ↳ Comments and Notes / Hyperlinks | — | S | button | gap |
| Editing | | Sort & Filter | `data.sort-range` | L | menu | live |
| Editing | | ↳ A→Z / Z→A / Custom Sort… | proxies of `data.sort-range.*` | S | button | live |
| Editing | | ↳ Filter / Clear | proxies of `toolbar.filter` / `data.clear-all-filters` | S | toggle | live |
| Editing | | ↳ Reapply | — | S | button | gap |
| Editing | | Find & Select | `edit.find-replace` | L | menu | live |
| Editing | | ↳ Replace… / Go To… / Select All / Notes | `edit.replace` `edit.go-to` `edit.select-all` `edit.select-commented` | S | button | live |
| Editing | | ↳ Go To Special… / Selection Pane | — | S | button | gap — §8 |
| Add-ins | 1 | Add-ins | — | L | button | gap |
| Analysis | 2 | Analyze Data | — | L | button | gap |

**Home's collapse order, authored:** Add-ins → Analysis → Editing → Cells →
Styles → Clipboard → Number → Alignment → Font. Dead-weight gap groups fold
first because their content is unavailable; then the occasional verb groups;
then the formatting groups that carry the frequent work, Font last. That is the
shape on every tab.

**The two number-format lists are unified here, and the ids are assigned by
hand.** Menu ▸ Format ▸ Number offers nine formats
(`webapp/editor.core.js:9869`); the toolbar's `#numfmt-menu` offers fifteen
(`webapp/editor.html:406`). Accounting, Long Date, Number (0), Thousands
(#,##0.00) and Percent (0.00%) exist only on the toolbar. The ribbon draws one
15-entry list — and **`commandId()` may not be allowed near it**, because it
slugifies labels (`webapp/editor.selection.js:1015`) and "Thousands (#,##0)" and
"Thousands (#,##0.00)" both slug toward `thousands-0`. Two lists for one concept
is exactly the drift a ribbon should collapse, and a slug collision is exactly
how collapsing it would go wrong.

### 3.4 Insert

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Tables | 11 | PivotTable | `insert.pivottable` | L | split | live |
| Tables | | ↳ From Table/Range | `data.pivottable-fields` (alias) | S | button | live |
| Tables | | ↳ From External / From Data Model; Recommended PivotTables | — | L | button | gap |
| Tables | | Table | `insert.table` | L | button | live |
| Tables | | Convert to Range | `table.convert-to-range` | S | button | live (context-menu only today) |
| Illustrations | 5 | Pictures, Shapes, Icons, 3D Models, SmartArt, Screenshot | — | L | menu | gap — §8 |
| Controls | 4 | Checkbox | — | L | button | gap |
| Charts | 10 | Recommended Charts | — | L | button | gap |
| Charts | | Column / Bar | `insert.chart.column` `insert.chart.bar` | S | menu | live |
| Charts | | Line / Area | `insert.chart.line` `insert.chart.area` | S | menu | live |
| Charts | | Pie / Doughnut | `insert.chart.pie` `insert.chart.doughnut` | S | menu | live |
| Charts | | Scatter | `insert.chart.scatter` | S | menu | live |
| Charts | | Hierarchy, Statistic, Waterfall/Funnel/Stock/Surface/Radar, Combo, Maps, PivotChart | — | S | menu | gap |
| Charts | | *launcher* → Chart panel | `openPanel("chart")` | | | live |
| Tours | 1 | 3D Map | — | L | split | gap |
| Sparklines | 6 | Line / Column / Win-Loss | — | M | button | gap |
| Filters | 7 | Slicer / Timeline | — | M | button | gap |
| Links | 9 | Link | `insert.hyperlink` | L | split | live |
| Links | | ↳ Edit Link… | `insert.hyperlink.edit` | S | button | live (context-menu only today) |
| Links | | ↳ Recent Items / Link to This Cell | — | S | menu | gap |
| Comments | 8 | Comment | `insert.note` | L | button | live |
| Comments | | Show Comments | `insert.note.show` | S | toggle | live |
| Text | 3 | Text Box, Header & Footer, WordArt, Signature Line, Object | — | L | menu | gap |
| Symbols | 2 | Equation / Symbol | — | L | split | gap |

### 3.5 Page Layout

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Themes | 1 | Themes / Colors / Fonts / Effects | — | L / M | gallery | gap — **not** `view.theme`, §8 |
| Page Setup | 5 | Margins, Orientation, Size, Print Area, Background, Print Titles | — | L | gallery / menu | gap |
| Page Setup | | Breaks | `file.page-break-here` | L | menu | live |
| Page Setup | | *launcher* → Page setup panel | `file.page-setup` | L | button | live |
| Scale to Fit | 3 | Width / Height / Scale | — | M | combo / spinner | gap |
| Sheet Options | 4 | Gridlines: View | `view.gridlines` | S | checkbox | live |
| Sheet Options | | Headings: View | `view.cell-markings` | S | checkbox | live |
| Sheet Options | | Gridlines: Print / Headings: Print / Sheet RTL | — | S | checkbox | gap |
| Arrange | 2 | Bring Forward, Send Backward, Selection Pane, Align, Group, Rotate | — | M | split / menu | gap |

`file.page-break-here` is placed in Excel's Breaks slot even though its natural
feedback surface — Page Break Preview — is a gap. That is stated rather than
hidden: we are drawing a command whose effect the user currently cannot see.

### 3.6 Formulas

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Function Library | 6 | Insert Function | `formulas.insert-function` (`fx-insert`, `webapp/editor.html:532`) | L | button | live — id must be minted |
| Function Library | | AutoSum | `formulas.autosum` | L | split | gap-with-a-verb |
| Function Library | | Recently Used, Financial, Logical, Text, Date & Time, Lookup, Math & Trig, More Functions | — | L / M | menu | gap — §8, the largest single group of missing controls |
| Python | 1 | Insert Python, Reset, Editor, Initialization | — | L / M | button | gap |
| Defined Names | 4 | Name Manager | `tools.name-manager` | L | button | live |
| Defined Names | | Define Name | `formulas.define-name` | M | split | live (context-menu only today) |
| Defined Names | | Use in Formula | `name-box-list` (`webapp/editor.html:528`) | M | menu | live — id must be minted |
| Defined Names | | Create from Selection | — | M | button | gap |
| Formula Auditing | 5 | Trace Precedents / Dependents / Remove Arrows | `format.trace.trace-precedents` `.trace-dependents` `.clear-trace-arrows` | M | button / split | live |
| Formula Auditing | | Show Formulas | `view.formulas-instead-of-results` | M | toggle | live |
| Formula Auditing | | Error Checking, Evaluate Formula, Watch Window | — | M / L | button | gap — §8 |
| Calculation | 3 | Calculation Options | `tools.calculation` | L | menu | live |
| Calculation | | ↳ Automatic / Manual | `tools.calculation.automatic` `.manual` | S | toggle | live |
| Calculation | | Calculate Now (F9) | `tools.calculation.calculate-now` | M | button | live |
| Calculation | | Automatic Except Data Tables; Calculate Sheet | — | S / M | — | gap |
| Solutions | 2 | Euro Conversion / Formatting / Quick Conversion | — | L / M | button | gap |

### 3.7 Data

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Get & Transform | 2 | Get Data, From Text/CSV, From Web, From Table/Range, From Picture, Recent Sources, Existing Connections | — | L / M | menu | gap — §8 |
| Queries & Connections | 1 | Refresh All | `data.refresh-all-pivots` | L | split | live |
| Queries & Connections | | Queries pane, Properties, Workbook Links | — | M | — | gap |
| Data Types | 3 | Stocks / Currencies / Geography | — | M | button | gap |
| Sort & Filter | 8 | Sort A to Z / Z to A | `data.sort-range.a-z` `.z-a` | S | button | live |
| Sort & Filter | | Sort | `data.sort-range.custom-sort` | L | button | live |
| Sort & Filter | | Filter | `toolbar.filter` (+ `data.filter` alias) | L | toggle | live |
| Sort & Filter | | Clear | `data.clear-all-filters` | M | button | live |
| Sort & Filter | | Reapply / Advanced | — | M | button | gap |
| Data Tools | 7 | Text to Columns | `data.text-to-columns` | M | button | live |
| Data Tools | | Convert text to numbers | `data.convert-text-to-numbers` | M | button | live — **ours, §8** |
| Data Tools | | Remove Duplicates | `data.remove-duplicates` | M | button | live |
| Data Tools | | Data Validation | `data.data-validation` | M | split | live |
| Data Tools | | Flash Fill, Circle Invalid Data, Consolidate, Relationships, Manage Data Model | — | M | button | gap |
| Forecast | 4 | What-If Analysis (Goal Seek, Scenario Manager, Data Table), Forecast Sheet | — | L | menu | gap — §8 |
| Outline | 6 | Group | `data.group.group-rows` | L | split | live |
| Outline | | ↳ Group Columns | `data.group.group-columns` | S | button | live |
| Outline | | Ungroup | `data.group.ungroup-rows` | L | split | live |
| Outline | | ↳ Ungroup Columns | `data.group.ungroup-columns` | S | button | live |
| Outline | | Show Detail / Hide Detail | `data.group.expand-all` `.collapse-all` | S | button | live |
| Outline | | Show level 1 / level 2 | `data.group.show-level-1` `.show-level-2` | S | button | live — **ours**, Excel draws these in the gutter |
| Outline | | Auto Outline, Clear Outline, Subtotal | — | S / L | button | gap |
| Analysis | 5 | Column stats… | `data.column-stats` | L | button | live — **ours, §8** |
| Analysis | | Data Analysis / Solver | — | L | button | gap |

### 3.8 Review

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Proofing | 1 | Spelling, Thesaurus, Workbook Statistics | — | L | button | gap |
| Accessibility | 2 | Check Accessibility, Alt Text | — | L | split | gap |
| Insights | 3 | Smart Lookup | — | L | button | gap |
| Language | 4 | Translate | — | L | button | gap |
| Language | | Language | `review.language` (`#locale-select`, `webapp/editor.html:604`) | M | combo | live, host-gated |
| Comments | 6 | New Comment | `insert.note` (proxy) | L | button | live |
| Comments | | Show Comments | `insert.note.show` (proxy) | L | toggle | live |
| Comments | | Select commented cells | `edit.select-commented` (proxy) | M | button | live |
| Comments | | Delete / Previous / Next Comment | — | L | button | gap |
| Notes | 5 | Notes | — | L | menu | gap |
| Protect | 7 | Protect Sheet | `format.protection.protect-this-sheet` | L | button | live |
| Protect | | Lock Cell | `format.protection.locked` | L | toggle | live |
| Protect | | Hide formula | `format.protection.hide-formula` | L | toggle | live — **ours, §8** |
| Protect | | Protect Workbook, Allow Edit Ranges, Unshare | — | L | button | gap |

### 3.9 View

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Sheet View | 3 | Exit | `data.clear-my-view` | M | button | live — **ours**, and it is the Exit half only |
| Sheet View | | Switch / New / Keep / Options | — | M | — | gap |
| Workbook Views | 4 | Normal, Page Break Preview, Page Layout, Custom Views | — | L | toggle | gap — §8 |
| Show | 5 | Gridlines | `view.gridlines` (proxy) | S | checkbox | live |
| Show | | Headings | `view.cell-markings` (proxy) | S | checkbox | live |
| Show | | Zero values | `view.zero-values` | S | checkbox | live — **ours**, Excel buries it in Options ▸ Advanced |
| Show | | Ruler / Formula Bar | — | S | checkbox | gap — §8 |
| Zoom | 6 | Zoom | `view.zoom` | L | menu | live |
| Zoom | | ↳ 50 / 75 / 100 / 150 / 200 % | `view.zoom.50` … `.200` | S | toggle | live |
| Zoom | | 100% | `view.zoom.100` | L | button | live |
| Zoom | | Zoom to Selection | — | L | button | gap |
| Window | 7 | Freeze Panes | `view.freeze` | M | menu | live |
| Window | | ↳ up to selection / top row / first column / unfreeze | `view.freeze.up-to-selection` `.top-row` `.first-column` `.unfreeze` | S | button | live |
| Window | | New Window, Arrange All, Split, Hide/Unhide, Side by Side, Sync Scrolling, Reset Position, Switch Windows | — | S / M / L | — | gap |
| Macros | 1 | Macros | — | L | split | gap — §8 |
| Collaboration | 2 | Version History | `file.version-history` (proxy) | L | button | live — **ours** |
| Collaboration | | Collaborators | `#presence` (proxy) | L | menu | live — **ours** |

### 3.10 Help

| group | ord | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- | --- |
| Help | 1 | Keyboard shortcuts | `help.keyboard-shortcuts` | L | button | live — **ours**, Excel links out |
| Help | | Help (F1), Contact Support, Feedback, Show Training, What's New, Community | — | L | button | gap |

**The shortcuts dialog's fifteen rows (`webapp/editor.core.js:9548`) do not
fully match the key map actually bound (`webapp/editor.core.js:8256-8660`).**
The ribbon's tooltip layer reads from the **key map**, never from that dialog.
Reconciling the two is a separate row, not ribbon work.

### 3.11 Contextual tabs

Three are buildable. **Picture Format and Shape Format are not built** — nothing
in this editor can select a picture or a shape (§8).

**Table Design** — trigger: a cell inside a table.

| group | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- |
| Tools | Summarize with PivotTable | `table.summarize-pivot` | L | button | live |
| Tools | Remove Duplicates | `data.remove-duplicates` (proxy) | L | button | live |
| Tools | Convert to Range | `table.convert-to-range` | L | button | live |
| Tools | Insert Slicer | — | L | button | gap |
| Table Style Options | Total Row | `table.totals-row` | S | checkbox | live |
| Table Style Options | Filter Button | `table.filter-button` | S | checkbox | live |
| Table Style Options | Header Row, Banded Rows/Cols, First/Last Column | — | S | checkbox | gap |
| Properties | Table Name, Resize Table | — | M | combo / button | gap |
| External Table Data | Export, Refresh, Properties, Open in Browser, Unlink | — | L / M | — | gap |
| Table Styles | Table Styles | — | L | gallery | gap |

**Chart Design** — trigger: a chart object selected.

| group | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- |
| Chart Layouts | Add Chart Element | `chart.add-element` | L | menu | live |
| Chart Layouts | Quick Layout | — | L | gallery | gap |
| Chart Styles | Change Colors, Chart Styles | — | L | gallery | gap |
| Data | Select Data | `chart.select-data` | L | button | live |
| Data | Switch Row/Column | — | L | button | gap |
| Type | Change Chart Type | `chart.change-type` | L | button | live |
| Location | Move Chart | — | L | button | gap |

**PivotTable Analyze** — trigger: a cell inside a pivot.

| group | control | command id | size | kind | status |
| --- | --- | --- | --- | --- | --- |
| Data | Refresh | `data.refresh-pivot` | L | split | live |
| Data | ↳ Refresh All | `data.refresh-all-pivots` (proxy) | S | button | live |
| Data | Change Data Source | `pivot.change-source` | L | split | live |
| Show | Field List | `data.pivottable-fields` | L | toggle | live |
| Show | +/− Buttons, Field Headers | — | L | toggle | gap |
| PivotTable | Name, Options | — | M | combo / split | gap |
| Active Field | Active Field, Field Settings, Expand/Collapse Field | — | M / S | — | gap |
| Group | Group Selection, Ungroup, Group Field | — | M | button | gap — **not** `data.group`, §8 |
| Filter | Insert Slicer, Insert Timeline, Filter Connections | — | L | — | gap |
| Actions | Clear, Select, Move PivotTable | — | L | menu | gap |
| Calculations | Fields Items & Sets, OLAP Tools, Relationships | — | L | menu | gap |
| Tools | PivotChart, Recommended PivotTables | — | L | button | gap |

**PivotTable Design is not drawn.** Every one of its controls is a gap; a tab
with no live control should not ship. When Report Layout exists it gets a tab.

### 3.12 The count

**188 distinct commands in, 188 out.** 185 are placed in a tab, a group, a
position and a size. Three are non-commands whose homes are named rather than
assigned: `toolbar.status` (a `<span>` in `.bottom-bar`,
`webapp/editor.html:597`, which acquires a `toolbar.*` id only because the boot
sweep stamps every `[id^='tb-']` node — `webapp/editor.core.js:11019` — and
`runCommand` would `.click()` a span); `toolbar.more` and `toolbar.more-flyout`,
which become the Simplified ribbon's shared trailing overflow and belong to no
tab.

**~60 further verbs that exist today with no command id at all** are given
Excel slots above and must have ids minted as part of the build: the whole
context-menu layer (`cellMenu` `webapp/editor.dialogs.js:2451`, `headerMenu`
`:2792`, `sheetMenu` `:2295`), `fx-insert`, `name-box-list`, the three un-id'd
`.tb-align` buttons (`webapp/editor.html:316`, `:319`, `:322`) and `autoSum`.
That is the second-largest win in the round: those verbs move from *ungoverned*
to inside `applyCommandRules()`, the read-only whitelist and the capability
gates.

**Against that, roughly 190 Excel controls are drawn as gaps.** The ribbon is
about half live, and drawing it complete is what makes that number visible
instead of arguable.

---

## 4. The Simplified ribbon, and which layout is the default

### 4.1 The default is Simplified

**Simplified + Always show.** Reasons, in order of weight:

1. **It is what was asked for.** The owner's words were "same ribbon / flatten
   ribbon".
2. **It is 18 px cheaper than today's chrome** (§1.2), where Classic costs 34 px
   more. The content-share advantage `docs/12` banks is enlarged, not spent.
3. **It is the layout Microsoft ships on the web**, and this product's primary
   mount is a browser tab.
4. **LibreOffice's default is a menu bar plus single-row toolbars** — roughly
   Simplified's height — while its Tabbed variant is a three-row ribbon it does
   *not* default to. Defaulting to Simplified takes the tab model's addressing,
   which is what buys inventory parity, at the height that precedent shows is
   sufficient.

Classic is one click away on the display-options caret and the choice persists.

### 4.2 Mechanics

One row, 32 px controls at a 32 px pitch, in a 40 px body. No group captions,
no group hairlines, no dialog launchers — each launcher's dialog moves into its
lead control's caret menu. Every control is a 16 px icon; only
`toolbar.font`, `toolbar.size`, `toolbar.numfmt` and `review.language` keep
visible text, because in each case the text **is** the value. Group order still
orders the row. Related commands merge behind the caret of the group's lead
control — Excel's documented rule and its own worked example (Align Left is
drawn; the other eleven alignment verbs live behind its caret).

**The row is authored, not measured.** A command never changes position between
two widths; it only ever falls off the end into the trailing `⋯` More Options
(`toolbar.more`, `webapp/editor.html:488`), which presents the dropped commands
**with labels** in their original group order — `webapp/editor.css:397` already
records why: three unlabelled glyphs in a floating box are a guessing game.

### 4.3 The per-tab rows

**Home — 25 slots, drawn at 1280 px and above**

| # | control | merged behind its caret |
| --- | --- | --- |
| 1 | `edit.paste` | paste-special, values, formulas, formats, transpose, cut, copy |
| 2 | `toolbar.painter` | — |
| 3 | `toolbar.font` (text) | — |
| 4 | `toolbar.size` (text) | — |
| 5 | `toolbar.bold` | — |
| 6 | `toolbar.italic` | — |
| 7 | `toolbar.underline` | strike, superscript, subscript, size-up, size-down |
| 8 | `toolbar.fontcolor` | swatch underline |
| 9 | `toolbar.fillcolor` | swatch underline |
| 10 | `toolbar.border` | 17 placements, 6 styles, 8 colours |
| 11 | `format.alignment.left` | the other 3 horizontal + 5 special + 5 vertical, rotate ×6, indent ±, valign |
| 12 | `toolbar.wrap` | wrap / overflow / clip |
| 13 | `toolbar.merge` | center, across, all, none |
| 14 | `toolbar.numfmt` (text) | the unified 15-entry list + custom |
| 15 | `toolbar.currency` | percent, comma, inc-dec, dec-dec |
| 16 | `format.conditional-formatting` | manage rules |
| 17 | `home.styles.format-as-table` | drop-gallery |
| 18 | `format.cell-styles` | drop-gallery |
| 19 | `insert.rows-above` | cells, rows-below, columns ×2, sheet |
| 20 | `insert.delete-rows` | delete-cells, delete-columns, delete-sheet |
| 21 | `home.cells.format` | 14 size / hide / sheet verbs |
| 22 | `formulas.autosum` | average, count, max, min, more |
| 23 | `data.sort-range` | A→Z, Z→A, custom, filter, clear |
| 24 | `edit.find-replace` | replace, go to, select all, select commented |
| 25 | `toolbar.more` | ⋯ |

**Insert — 11 slots**: `insert.pivottable` · `insert.table` ·
`insert.illustrations` *(gap)* · `insert.checkbox` *(gap)* · `insert.chart` ·
`insert.sparkline.line` *(gap)* · `insert.slicer` *(gap)* ·
`insert.hyperlink` · `insert.note` · `insert.text` *(gap)* · `⋯`.

**Page Layout — 9 slots**: Themes *(gap)* · Margins *(gap)* · Orientation
*(gap)* · Size *(gap)* · Print Area *(gap)* · `file.page-break-here` ·
`file.page-setup` · Sheet Options (`view.gridlines` + `view.cell-markings`
merged) · `⋯`.

**Formulas — 9 slots**: `formulas.insert-function` · `formulas.autosum` ·
Function categories *(gap)* · `tools.name-manager` · `formulas.define-name` ·
`format.trace` · `view.formulas-instead-of-results` · `tools.calculation` ·
`⋯`.

**Data — 10 slots**: `data.get-data` *(gap)* · `data.refresh-all-pivots` ·
`data.sort-range.a-z` · `data.sort-range.z-a` · `data.sort-range.custom-sort` ·
`toolbar.filter` · `data.text-to-columns` · `data.group.group-rows` ·
`data.column-stats` · `⋯`.

**Review — 7 slots**: Spelling *(gap)* · Check Accessibility *(gap)* ·
`insert.note` · `insert.note.show` ·
`format.protection.protect-this-sheet` · `review.language` (text) · `⋯`.

**View — 8 slots**: `view.normal` *(gap)* · `view.gridlines` ·
`view.zoom` · `view.zoom.100` · `view.freeze` · `data.clear-my-view`
(labelled "Exit") · `file.version-history` · `⋯`.

**Help — 3 slots**: Help *(gap)* · `help.keyboard-shortcuts` · `⋯`.

**Contextual**: Table Design — summarize, remove-duplicates, to-range,
totals-row, filter-button, `⋯`. Chart Design — add-element, change-type,
select-data, `⋯`. PivotTable Analyze — refresh, change-source, field-list, `⋯`.

### 4.4 What the layout toggle must not change

The tab set, the KeyTip letters, contextual-tab behaviour, which tab is active,
and where focus is. Switching layout must not reflow the grid by more than the
body delta (41 ⇄ 93), and that delta goes through `resize()` **exactly once**,
at `transitionend`, animated over the same 220 ms as a manual collapse.

---

## 5. The collapse contract

### 5.1 The principle

Today's collapse is **computed**: `reflowToolbar()` expands everything, then
folds groups one at a time until `fits()` — literally
`toolbarEl.scrollWidth <= toolbarEl.clientWidth + 1`
(`webapp/editor.core.js:9370`, `:9402`). Nobody chose 1461 px; it is what fell
out of the arithmetic, and `docs/88` §1.4 measured the consequence — a 1440 px
laptop was already degraded and at 1280 a third of the bar was chips.
`docs/88` §3.5 quotes Harris on what Office actually does: *"the order in which
the chunks collapse into different versions is also designed by us and not by
the computer. There's no attempt to 'auto-scale' the UI."*

**So: every breakpoint below is authored. There is no measurement loop.** Each
group declares an ordered list of variants and the width at which it steps
down. A command therefore never changes its position on the row between two
widths — it only ever falls off the end into the `⋯`, which is the invariant
that makes the row learnable.

### 5.2 The budget

Computed from §2's metrics. **Not measured** — §10's `UX-RIB-05` gate is how it
becomes a measurement.

Simplified Home, every cluster at full variant, 32 px pitch, 0 intra gap, 8 px
inter-cluster, 44 px per split (32 + 12):

```
Clipboard 44 · Font combos 188 · B I U 108 · colours 88 · borders 44
· alignment 44 · number format 96 · number quick-set 172
· conditional formatting 44 · cell styles 44 · Cells 132 · Editing 220
= 1224 px of controls
+ 11 boundaries × 8            =   88
+ bar padding                  =   16
+ pinned trailing (⋯ 32, caret 24, gap 6) = 62
= 1390 px required
```

Classic Home, the same inventory in three rows with 13 px ruled boundaries:
**≈1180 px**, because three rows absorb the width one row spends.

### 5.3 The contract

| viewport | Simplified (default) | Classic |
| --- | --- | --- |
| **2560** | full row, 1170 px slack; the row is **left-aligned, not stretched** — distributing 1170 px between twelve clusters puts Bold 900 px from Paste | full, left-aligned |
| **1920** | full row, 530 px slack | full |
| **1600** | full row, 210 px slack | full |
| **1440** | full row, **50 px slack** | full, 260 px slack |
| **1366** | step 1: **Editing** → labelled `Editing ⌄` chunk (−176) → 1214 px | full, 186 px slack |
| **1280** | step 2: **Styles** → `Styles ⌄` (−88) → 1126 px | step 1: Editing → chunk, 1094 px |
| **1024** | steps 3-4: **Cells** → `Cells ⌄` (−132), **Number quick-set** → folds behind the number-format caret (−172) → 822 px | steps 2-3: Styles, then Cells → 862 px |
| **768** | step 5: **Borders + colours** → absorbed by `Font ⌄` (−132); font combos shrink to 96/40 (−52) → 638 px | **forced to Simplified at ≤900 px** — three 24 px rows plus a caption in a 768 px window is a ribbon that is mostly caption |
| **≤720** | **the ribbon body does not render.** The tab strip becomes one overflow control and commands are reached through the hidden menu tree's `⋯`. Below 560 px the row would be 124 px of a 640 px viewport, which is not a trade worth making | n/a |
| **560 / 412 / 390 / 320** | Simplified forced regardless of the persisted preference — **overridden, not overwritten**, the pattern `embed.js:283` uses for `access: "preview"`; every cluster is a labelled chunk; the row scrolls within itself with a pinned `⋯`; the tab strip scrolls with pinned ◀ ▶ | n/a |

**Authored overflow order for Home's Simplified row** — first pushed into the
`⋯`, in this exact sequence, never re-measured and never reordered:

`format.cell-styles` → `home.styles.format-as-table` →
`format.conditional-formatting` → `toolbar.painter` → `toolbar.border` →
`toolbar.fillcolor` → `toolbar.fontcolor` → `home.cells.format` →
`insert.delete-rows` → `insert.rows-above` → `formulas.autosum` →
`toolbar.merge` → `toolbar.wrap` → `toolbar.currency` → `toolbar.size` →
`toolbar.font`.

**Seven never leave the row at any width ≥320 px**: `edit.paste`,
`toolbar.bold`, `toolbar.italic`, `toolbar.underline`,
`format.alignment.left`, `toolbar.numfmt`, `toolbar.more`. That is the 320 px
floor, and it is the assertion the budget gate carries.

### 5.4 What never collapses, at any width, in any state

- The tab strip's existence. It scrolls, with ◀ ▶ **pinned outside the scroll
  box on the left** — the only placement that cannot itself be pushed off, which
  is exactly the defect `docs/88` §5 found in the sheet strip.
- The File tab and the active tab's identity.
- The QAT's Save / Undo / Redo.
- The formula bar's Name Box and input.
- `#tb-status` in the status bar.
- **Every command's DOM node.** A collapsed cluster's controls **move** into its
  chunk panel; they are never destroyed and never cloned — the contract
  `collapseGroup`/`expandGroup` already keep
  (`webapp/editor.core.js:9361`). Likewise **all tab panels stay in the DOM**
  with the inactive ones `hidden`.

### 5.5 Why this does not reproduce the 1461 px defect

1. The first collapse is at **1366**, a width somebody chose, and 1440 carries
   50 px of headroom above it. The defect was never that a collapse existed; it
   was that the first one fired above every mainstream laptop because nobody had
   picked a number.
2. There is no `fits()` loop, so no width at which the row silently reorders.
   Between 1440 and 1367 the row is byte-identical.
3. The collapse unit is a **named, labelled chunk**, not a mystery glyph.
   Harris's fourth rule — a popup chunk shows the layout it would have had
   inline — is broken deliberately today because our toolbar has no labels to
   preserve (`docs/88` §3.5, `webapp/editor.css:381`). A ribbon group **has** a
   caption, so Classic can now honour it: the chunk renders the group's inline
   layout. Simplified's chunk stays a labelled vertical list, for the reason the
   existing comment gives.
4. `.tb-collapsed` chips as a *desktop* idiom disappear: at 1440 and above, on
   every tab, there are none.

### 5.6 Touch

Under `(pointer: coarse)` the Simplified row grows to 48 px with 44 × 44
controls — the heights the bands already go back to
(`webapp/editor.css:2196`), whose stated reason is that a 44 px button in a
42 px band overflows it. **Classic is not offered on a coarse pointer at all.**
`editor.touch-targets.spec.mjs:142` caps coarse-pointer band growth across
`.toolbar` + `.formula-bar` at 16 px total; a Simplified body starting at 40 and
growing to 48 is inside that cap, and Classic's would not be. That is the second
reason Classic is desktop-only, and the cap is a decision restated in §9 rather
than relaxed.

---

## 6. Backstage, and the decided fate of the menu bar

### 6.1 Backstage

**Two columns, full window, no tab strip.** It replaces everything from the top
of the window to the bottom of the grid; nothing of the editor shows through.
Concretely it is a sibling of `.work-area` in the same flex column
(`webapp/editor.css:228`), `flex: 1 1 auto`, with the chrome bands above it
`display: none` while it is open — so the column keeps working and `resize()`
has a real box to measure on the way out.

**Rail:** left, fixed 232 px at ≥1200 px and 200 px below, never a percentage. A
40 px circular Back arrow inset 12 px from the top-left; the destination list at
40 px rows, 16 px horizontal padding, 13/500; then a divider; then Account,
Feedback, Options pinned to the bottom with `margin-top: auto`. The rail is
filled with **`--oc-accent-color`** and `--oc-accent-contrast-color` text — this
is the one large accent surface in the product, which is why the rest of the
chrome stays neutral. It is **never a hard-coded green** (§8, §9). A host that
sets a brand accent gets a branded rail for free, which is the same reasoning
that made `--oc-accent-ring` derived rather than literal
(`webapp/editor.css:110`). The active row is a filled block spanning the rail's
full width with a 3 px leading bar, not an underline — an underline in a
vertical list reads as a divider.

**Content pane:** 40 px top and left padding, capped at 1040 px and
**left-aligned**, so the pane does not drift away from the rail at 2560 px. Page
heading 28/36 semibold. Every pane is built from three primitives and nothing
else — the **card**, the **two-column split** (1fr 1fr at ≥1280 px, one column
below 1024), and the **list row** (56 px, hover tints rather than lifts). That
is what keeps fifteen panes looking like one region.

At 1024 px the rail narrows to 200 px and every two-column pane becomes one
column. Below 900 px the rail becomes a full-width horizontal scroller above the
pane, **with the same labels**. It never becomes icons-only: a rail of
unlabelled glyphs is the guessing game `webapp/editor.css:381` already rejected.

**Rail order:** Home, New, Open, Info, Save *(desktop only)*, Save a Copy /
Download, History, Print, Share, Export, Publish, Close — divider — Account,
Feedback, Options. §3.2 assigns the commands.

**Two panes carry real behaviour that should be relocated rather than
rewritten.** *Info* is `documentPropertiesDialog()`
(`webapp/editor.dialogs.js:2879`) — its own two-block layout, five editable
fields then four read-only facts with the "—" rule, is already this pane.
*History* is `buildHistoryPanel()` (`webapp/editor.core.js:5208`) — capture,
IndexedDB persistence, the three naming tiers, the restore plan and the
hide-not-delete rule are all built. *Home*'s draft list is `listDrafts()`
(`webapp/editor.drafts.js:209`), already drawn with these four row actions into
the transient `#oc-recovery` band; Backstage Home is where that list belongs
permanently and the band stays for the boot-time offer.

**Save a Copy shows the loss report inline, above the button, before any bytes
exist** — not as a modal after the click. `confirmLoss()` /
`session_save_loss_for` (`webapp/editor.sheets.js:628`,
`crates/casual-calc-wasm/src/io.rs:349`) is already per-chosen-format, which is
what makes that possible.

**Every pane declares empty, loading and error.** Empty is a sentence naming
why, never a blank column. Loading is a skeleton of the pane's own list rows,
and only for the two panes that read IndexedDB. Error is inline in the pane and
never a toast: the store being unreadable is a fact about the pane you are
looking at.

**Interaction.** Opening: the File tab, `Alt` then `F`, or `runCommand("file")`.
It is a **route** — `?backstage=info`, back/forward-safe, filtered against a
known destination list before anything reaches a class, the way `?hide=` is
filtered against `CHROME_REGIONS` (`webapp/editor.core.js:850`); an unknown
destination is nobody having asked, and lands on Home. Closing: Back arrow,
`Escape`, or activating any ribbon tab (which closes **and** selects that tab in
one action). Save is the fourth — it commits and closes.

Focus lands on the Back arrow on entry and returns to the File tab on exit, or
to the newly-selected ribbon tab if the exit was a tab click — **never to the
grid**, because the user did not ask to be put back in a cell. It is a focus
trap: `role="dialog"`, `aria-modal="true"`, `aria-labelledby` on the pane
heading. That is right even though it is a route, because everything behind it
is `display: none` — a trap that fences off nothing is free, and without it Tab
walks into hidden chrome.

**Escape is layered and each press consumes one layer**: an open Options dialog
→ the backstage; an inline editor in Info → reverts and blurs to the pane;
otherwise → the grid. It must be captured on the backstage container and
stopped, because the editor's key handling runs on the document and the canvas;
`documentPropertiesDialog`'s `onKey` already takes that care
(`webapp/editor.dialogs.js:2957`).

**Underneath, nothing happens, and that is a requirement.** Selection, scroll
position, active sheet and calculation state are untouched, and closing restores
the viewport to the pixel. `resize()` is called **on exit and only on exit** —
calling it on entry, when `.work-area` is `display: none`, measures 0 × 0. **If
a cell editor was open when File was activated it is committed, not
abandoned**: entering the backstage is a document-level act, and leaving a
half-typed cell live behind a full-window surface is how an edit gets lost to an
Escape aimed at the backstage.

Collaboration and autosave keep running. The one visible consequence is that the
collaborator roster has no home while the backstage is open; it goes in the
backstage's own top-right, above the pane heading, moved by the **same
reversible mechanism** the desktop chrome already uses (`placeNativeChrome()`
and the `chromeHome` WeakMap, `webapp/editor.core.js:1259`, `:1310`). Do not
write a second relocation.

### 6.2 The menu bar: hidden-but-present. Decided.

**The eight `.menu-top` buttons and their eight `.menu-drop` panels stay in the
DOM, unchanged, and the bar is hidden by a class in ribbon chrome.** This is not
a compromise: it is the state the desktop shell has shipped in for months —
`.oc-chrome-native.oc-native-menu #menubar { display: none }`
(`webapp/editor.css:2055`) hides the bar today and the OS menu is complete,
which is the empirical proof that a `display:none` menu bar publishes a full
model.

The mechanism, read rather than assumed:

- `menuModel()` reads `qsa("#menubar .menu-top")` and filters on
  `b.dataset.ocCommand && !ruleHidden(b)` — the `.oc-cmd-hidden` class, **not**
  visibility and not the `hidden` attribute
  (`webapp/editor.selection.js:1128-1131`). `querySelectorAll` does not consult
  layout.
- The `.menu-drop` panels are **not children of `#menubar` at all**:
  `buildMenuBar()` appends each to `ocOverlayHost`
  (`webapp/editor.core.js:10095`). Hiding the bar hides eight buttons and
  nothing else. All 144 leaf items keep their boxes, their
  `data-oc-command`, their `.mi-label` / `.mi-key` / `.mi-check` slots and their
  handlers.
- `listCommands()` filters on `.oc-cmd-hidden` too
  (`webapp/editor.selection.js:1040`), and its neighbouring comment says why:
  four controls are authored `hidden` and must stay runnable.

**Consequence for the native OS menu: none.** `menuModel()` returns the same
eight menus with the same leaves; the shell's readiness poll
`ready = !!(e && e.menuModel && e.menuModel().length)`
(`desktop/src/main.rs:927`) still flips; `on_menu_event` still evaluates
`runCommand('<id>')` against nodes that exist. **Consequence for
`listCommands()`: none.** Every id it lists today it lists after.

**Three costs, named rather than discovered later.**

1. **"Listed implies reachable by pointer" breaks unless the ribbon carries all
   144 leaves.** `editor.host-toolbar.spec.mjs:58` enforces the invariant only
   as *node existence*, which hidden-but-present satisfies vacuously. It must be
   strengthened to *painted after activating at most one tab* — `UX-RIB-06`.
   This is the single most important new gate in the round.
2. **The eight top-level ids become clickable ghosts.** `runCommand("file")`
   finds the `.menu-top`, is neither `disabled` nor `.oc-cmd-hidden`, and clicks
   it; `openMenu(i)` then calls `anchorMenu(drop, topBtns[i])`, which reads
   `getBoundingClientRect()` of a `display:none` element
   (`webapp/editor.core.js:8806`) — all zeros — and drops the menu in the
   window's top-left corner. This is latent on the desktop today and becomes
   reachable in a browser the day the ribbon ships. Fix in this programme:
   re-point the eight top-level ids at the ribbon tab or backstage they now
   correspond to, or refuse `openMenu` when the bar has no box.
3. **Two definitions of one inventory.** `MENUS` (`webapp/editor.core.js:9611`)
   and the ribbon layout must not drift. The answer is a static gate asserting
   set equality **both ways** — the one-way version passes while the ribbon
   holds a stale id.

**And one thing that must move first.** `.oc-hide-menubar` is published host API
that hides the bar's *items* and keeps the collaborator roster
(`webapp/editor.css:267`, `webapp/editor.html:181`, `UX-CHROME-01`, asserted by
`editor.chrome-regions.spec.mjs:43`). Hiding the whole bar for the ribbon takes
the roster with it, so **the roster relocates to the title strip first**
(§1.4), and `.oc-hide-menubar` keeps its published meaning — `embed.js:262`
throws on unknown region names, so the name cannot change without a major
version.

**Rejected: deriving the OS menu from the ribbon.** A ribbon's grouping is
Home / Insert / Page Layout / Formulas; an OS menu bar is File / Edit / View.
Deriving one from the other gives macOS a menu bar whose top level is ribbon
tabs and whose second level is ribbon groups — which is not a menu bar, it is a
ribbon in a trench coat, and it would put Bold under "Home" where every Mac user
looks for it under "Format".

**Rejected outright: deleting the menu bar.** `menuModel()` returns `[]`, so the
shell's poll never satisfies, `opencalc-native-ready` never fires,
`.oc-native-menu` is never stamped by `nativeMenuIsDrawn()`
(`webapp/editor.core.js:1163`), and the desktop app boots with **no OS menu at
all** — and, because the two-class evidence gate never latches, with the HTML
bar it was supposed to have replaced also gone. The eight top-level ids leave
`listCommands()`, which is a published-surface break.

### 6.3 `listCommands()` and contextual tabs

Two hard constraints follow from R1, and the second is worse than the first
because no existing gate can catch it.

**Constraint A — no lazy tab rendering.** Covered in R1.

**Constraint B — contextual tabs and their controls are always in the DOM.** If
their commands exist only while a chart is selected, `listCommands()` becomes
**selection-dependent**: a host enumerating commands at load gets a different
answer than one enumerating after a click, and the SDK's published surface
changes size as the user clicks around. Unavailability is expressed as
`disabled`, through `applyCommandRules()`, never as absence.
`editor.host-toolbar.spec.mjs:58` cannot notice this — the node is absent from
the list too — so `UX-RIB-10` adds the snapshot assertion.

### 6.4 Host regions and the embed contract

The region **names** are frozen. The ribbon adds none. `toolbar: false` must
keep hiding whatever occupies the toolbar's role, so **the ribbon's selector is
added to `.oc-hide-toolbar`'s rule** (`webapp/editor.css:253`) rather than the
ribbon carrying `class="toolbar"`.

**Do not add `ribbon` to `ChromeRegions`.** A host that wrote
`{ toolbar: false }` and now gets a ribbon it cannot hide has had a region
silently appear in its product, and `embed.js:262` throws on unknown names so
old hosts cannot feature-detect. Adding a name is a major version.

**A pre-existing defect is inherited here and must not be fixed inside this
work.** `webapp/embed.d.ts:75-82` declares `ChromeRegions` as
`{ header, toolbar, formulaBar, statusbar, sheetTabs }`; `webapp/embed.js:97-99`
accepts `["header","menubar","toolbar","formulabar","tabs","statusbar","localePicker"]`
and throws on anything else. So `formulaBar` and `sheetTabs` are
declared-and-throwing, and `menubar` and `localePicker` are
supported-and-undeclared; `sdk/types/consumer.ts:45` exercises only `toolbar`
and `statusbar`, which is why the `sdk-types` gate does not catch it. That gets
its own row and its own change, **before** the ribbon work, because fixing it
inside a ribbon change makes the ribbon change the thing that broke a host's
build.

---

## 7. Keyboard, and ARIA

### 7.1 KeyTips — two levels

**Level 1 — tabs and the QAT.** `Alt` (or `F10`) paints a badge on every tab, on
File, on the Search box and on each QAT item. Excel's published tab access keys
are used verbatim:

`F` File · `H` Home · `N` Insert · `P` Page Layout · `M` Formulas · `A` Data ·
`R` Review · `W` View · `Q` Search

QAT items get **numeric** KeyTips — `Alt+1` … `Alt+9`, then two-digit for the
tenth onward. Numeric precisely so they can never collide with the alphabetic
tab letters, which is why Excel does it.

Contextual tabs use Excel's `J`-prefixed shape: `JT` Table Design, `JC` Chart
Design, `JA` PivotTable Analyze. **[unverified]** — Microsoft does not publish
contextual-tab access keys in the same table as the fixed ones; these are our
choice in Excel's shape.

**Level 2 — controls within the open tab.** Authored per tab, unique *within*
the tab, one or two letters. Excel's Home sequences as-is: `H,1` Bold · `H,2`
Italic · `H,3` Underline · `H,FF` Font name · `H,FS` Font size · `H,AL` Align
Left · `H,AC` Center · `H,AR` Align Right · `H,W` Wrap · `H,N` Number Format ·
`H,I` Insert · `H,D` Delete · `H,O` Format · `H,US` AutoSum · `H,SS` Sort &
Filter · `H,FD` Find & Select.

**KeyTip letters are explicit data in the layout manifest, asserted unique.
They are never derived.** The menu bar's Alt map today takes the first free
letter of the *translated* label and rebuilds on every relabel
(`webapp/editor.i18n.js:87-108`), which is right for a menu bar and wrong for a
KeyTip scheme: it would produce different letters than any user's muscle memory,
and different letters between two locales for the same tab. The letter is a
**per-locale table**, seeded from Excel's English letters and overridable through
the existing `setMessages` catalogue under a `keytip.<id>` key space alongside
`command.<id>` and `tip.<id>` (`webapp/editor.i18n.js:41`, `:124`). Uniqueness
is validated at catalogue load; a collision falls back to first-free-letter
rather than silently shadowing.

**Escape at any KeyTip level goes back exactly one level**, and at level 1
dismisses KeyTips and **returns focus to the cell that had it**. That last part
is new work, not a port: today's handler opens a menu and focuses its first item
with no record of where focus came from (`webapp/editor.core.js:10168`). The
origin is captured on Alt-down and restored on dismissal.

**Three consequences of adopting Excel's letters, stated rather than
discovered.** `Alt+V` was View and becomes `Alt+W`; `Alt+O` was Format and
becomes `Alt+H` (Home). Excel's letters win — the migrant is who this round is
for — but a changed letter is a change and belongs in the release note.
`aria-keyshortcuts` becomes per control, not per tab. And **the menu bar's Alt
mnemonics survive alongside**: the menu bar still exists, they are a different
surface, and deleting them is a regression for every existing user.
`editor.excel-shortcuts.spec.mjs:198` must stay green.

**A KeyTip advertised and dead is the same class of defect the advertised-chord
suite exists for.** The gate stubs `runCommand`, fires the sequence, and
**compares the id** (`editor.excel-keyboard-parity.spec.mjs:349`). "The key did
something" does not count.

**`desktop/src/menu.rs`'s `releases_during_edit()` must be audited against the
new letters** before the desktop slice merges: a native accelerator is consumed
before the webview sees the key.

### 7.2 Ctrl+F1

Toggles the ribbon body between the persisted display state and `tabs`.
Double-clicking the active tab does the same. Neither touches the Classic ⇄
Simplified axis. **Verified free**: the only F-key bindings in
`webapp/editor.core.js` are F9 at `:8645` and Shift+F11 at `:8650`.

### 7.3 F6 landmark order — entirely new work

There is nothing to preserve. A grep of `webapp/` for `F6` returns zero hits in
source; `docs/88` cites Excel's F6 order only as a competitor fact. Region-to-
region movement today is Tab plus `Ctrl+G` / `F5` into the Name Box.

Forward order, `Shift+F6` reversing:

1. **Ribbon** — the tab strip; Tab from there enters the body
2. **Formula bar** — the Name Box
3. **Grid** — the canvas
4. **Side panel**, when open
5. **Sheet-tab strip**
6. **Status bar**
7. **Title / QAT strip** → wraps to 1

It starts at the ribbon rather than at the top of the window because F6 exists
to get *out of the grid and to the commands*, and Excel's own order starts
there. Each landmark takes focus on the element last focused inside it, falling
back to its first control, so F6-ing away and back does not lose the user's
place. The side panel is in the cycle only when open — a landmark that is
present but empty is a dead press.

### 7.4 Arrow roving

**Tab strip.** Left/Right move selection **and activate** — automatic
activation, because all panels are already in the DOM so activation is free, and
it is what Excel does. Home/End to the ends. `Down`, `Enter` or `Space` moves
focus into the body's first control **without** changing selection. The strip
scrolls the focused tab into view.

**Ribbon body.** **One Tab stop for the whole body**, extending the
composite-control contract the toolbar already keeps
(`webapp/editor.core.js:9433-9476`). Within it: Left/Right move between controls
and **cross group boundaries** — this differs from Excel, which stops at group
edges, and the difference is deliberate, because our `items()` is a flat list
(`webapp/editor.core.js:9441`) and crossing is what users of *this* editor
already have, so stopping would be a regression introduced by a rewrite.
Up/Down move between Classic's three rows in the nearest column position; in
Simplified, `Down` on a split or menu button **opens its menu**. Home/End go to
the first/last control **in the body**, not in the group. Text fields keep their
own Left/Right — the carve-out at `webapp/editor.core.js:9461`.

**`syncStops()` must sweep every control in every panel, not just the active
one.** The existing sweep resets `tabIndex = -1` on controls parked inside a
closed flyout (`webapp/editor.core.js:9453`), because they would otherwise
become extra tab stops the moment the flyout opened. Hidden tab panels reproduce
that trap at n times the scale.

**Escape.** From a control → the active tab button. From a tab button → the
grid, restoring the previous selection. From an open menu → its trigger. From
Backstage → the File tab. Never two levels at once.

**Menus and galleries.** Up/Down within rows, Left/Right to the adjacent group's
menu (matching `webapp/editor.core.js:10184`), Home/End to the ends, type-ahead
on first letter. A gallery is a grid: Left/Right within a row, Up/Down between
rows.

**Submenus open on hover only where hovering exists** —
`matchMedia("(hover: hover) and (pointer: fine)")` checked **per event**, not
once at build time, because Chrome replays `mouseenter` before `click` at a
touch point (`webapp/editor.core.js:10046`). **A click inside a flyout must not
dismiss it** (`webapp/editor.core.js:9430`) — ribbon dropdowns hold live inputs
(the font box, a hex field), and a naive outside-click dismissal closes the
panel out from under the control being typed into.

### 7.5 ARIA

**Tab strip** — `role="tablist"` `aria-label="Ribbon"`
`aria-orientation="horizontal"`. The label must **not** be "Sheets": the sheet
strip already owns `role="tablist" aria-label="Sheets"`
(`webapp/editor.html:587`), and two tablists in one application need distinct
correct labels or a screen-reader user cannot tell which one they landed in. The
sheet tablist is not re-pointed.

**Each tab** — `role="tab"`, `id`, `aria-selected`, `aria-controls`, roving
`tabindex` with exactly one `0` in the strip at all times.

**The File tab is not a tab** — `role="button"` `aria-haspopup="dialog"`
`aria-expanded`. It opens a surface that replaces the whole window; putting it
in the tablist would make `aria-selected` lie and let arrow keys rove into it.

**Body** — one `role="tabpanel"` per tab with `aria-labelledby` and
`tabindex="-1"` so F6 and the strip's `Down` can place focus. Inactive panels
carry `hidden` + `aria-hidden="true"` **and an explicit
`[hidden] { display: none }` rule** — `hidden` is an attribute and loses to any
author `display:` declaration, a trap this stylesheet has sprung five times
(`webapp/editor.css:375`, `:386`, `:449`, `:541`, `:1972`), and the note at
`:541` is the relevant one: a hidden menu button still occupied the width being
*measured*.

**Group** — `role="group"` `aria-label="<caption>"`. In Simplified the caption
is not drawn but the label stays: the grouping is still true, only visually
unlabelled.

**Split button** — a wrapper `role="group"` containing **two real
`<button>`s**. Primary `aria-label="Paste"`; caret `aria-label="Paste options"`
`aria-haspopup="menu"` `aria-expanded`. Two names because they are two commands,
and **both halves need their own command id** or one is undispatchable.

**Toggle** — `<button aria-pressed>`. Not `role="switch"`: a ribbon toggle is a
formatting command with a latched state, not a setting.

**Menu button** — `aria-haspopup="menu"` `aria-expanded`; panel `role="menu"`;
rows `role="menuitem"` / `menuitemcheckbox` / `menuitemradio`. **Rows keep the
three-slot `.mi-check` / `.mi-label` / `.mi-key` structure**
(`webapp/editor.core.js:10058`): `menuModel()` reads the label from `.mi-label`
and the accelerator from `.mi-key`, `refreshChecks()` writes the tick into
`.mi-check`, `relabel()` writes translations into `.mi-label`. Change the row
markup and the OS menu loses its labels, accelerators and ticks at once.
**Labels are set with `textContent`, never `innerHTML`** — a translated label is
host-supplied text (`SEC-001`, `webapp/editor.core.js:10060`,
`webapp/editor.i18n.js:99`).

**Gallery** — `role="listbox"` with a label; items `role="option"`
`aria-selected`. An in-ribbon gallery showing a window onto a longer list adds
`aria-setsize` / `aria-posinset`, so "3 of 42" is announced rather than "3 of 3".

**Combo box** — `role="combobox"` `aria-expanded` `aria-controls` on the input,
`role="listbox"`/`option` on the popup, `aria-activedescendant` while
navigating — what `wireCombo` already builds for font and size
(`webapp/editor.core.js:9114`, `:9129`). **The number-format readout must be
built the same way**, not as a menu button with a text span: today
`#tb-numfmt-label` is a `<span>` stamped as a command by the boot sweep, and
`runCommand` would `.click()` a span.

**Contextual tab** — `role="tab"` plus `aria-describedby` naming the trigger.
Its appearance is announced through the existing `#grid-live` region
(`webapp/editor.html:559`), **never** by putting `aria-live` on the tablist — a
live region on a tablist announces every arrow press.

**Backstage** — `role="dialog"` `aria-modal="true"` `aria-labelledby`; rail
`role="navigation"` around a `role="list"`; focus trapped; parented to
`ocOverlayHost` (`webapp/editor.core.js:1432`) — `document.body` for a page
mount, the **shadow root** for `<opencalc-sheet>` — never `document.body`
unconditionally, or an embedded editor's backstage renders outside the shadow
boundary with the host's stylesheet applying instead of ours.

**Disabled** — the `disabled` attribute, not `.oc-cmd-disabled`'s
`opacity: .45; pointer-events: none` alone. An opacity-dimmed button is still in
the accessibility tree as actionable.

**Accessible names.** Every ribbon control has a non-empty one. The tooltip is
not a name: `initTooltips()` promotes `title` → `data-tip` **and `aria-label`**
for `.toolbar [title]` and three other ancestors *only*
(`webapp/editor.core.js:5724`), so a ribbon outside those four selectors loses
its names silently. Give every icon-only control an explicit `aria-label`; that
is safer than extending the selector list.

**Contrast.** The focus ring clears 3:1 against both the control's own fill and
the ribbon's background, measured the way
`editor.chrome-and-contrast.spec.mjs:56` measures gridlines.
`prefers-contrast: more` keeps applying (`webapp/editor.css:1886` lifts
`--oc-border-color` and `--oc-gridline-color`); the ribbon's group rules use
`--oc-border-color` and inherit that lift for free.

**No state is signalled by colour alone.**

**Command identity, which is an a11y and an SDK contract at once.** Every ribbon
control carries `data-oc-command` or `data-oc-proxy`. Conversely **no pure-chrome
element may carry an `id` starting `tb-`**, because the boot sweep stamps every
such node as a command (`webapp/editor.core.js:11019`) — that is how four
non-commands (`toolbar.status`, `toolbar.numfmt-label`, `toolbar.more-flyout`,
and the two combo carets) are in `listCommands()` today. And **menu command ids
must not be re-derived from the new tab path**: `commandId()` slugifies the
English label path (`webapp/editor.selection.js:1015`), `CAPABILITY_COMMANDS`
matches those ids by regex (`webapp/editor.core.js:1336`), and a regroup that
re-slugs `file.download.csv-csv` into `home.…` silently un-gates `canSaveAs`.

---

## 8. What this refuses

**1. It refuses inventory parity with Excel, and says so plainly.** Excel's
always-visible tabs alone carry well over 200 controls; our inventory is 188
commands. The gap is not a ribbon gap — it is Power Query, Office Scripts, VBA
and the Developer tab, ActiveX and Form controls, SmartArt, WordArt, Ink and the
Draw tab, Solver, the Analysis ToolPak, Sensitivity labels, Python in Excel,
3D Maps, Screenshot, Signature Line, Publish to Power BI, Smart Lookup,
Translate, Thesaurus. Drawing a control for a verb the engine cannot perform
gives two options and both are worse than omission: a dead button, or a
permanently-disabled one. **~150 permanently-disabled controls is a worse
product than a smaller ribbon**, because it advertises absence in 150 places on
every screen, forever. **What is built at exact parity is the structure**: the
tab set, the tab order, the group model and captions, the four control sizes,
dialog launchers, in-ribbon galleries, KeyTips, the backstage, the
Simplified/Classic toggle, contextual tabs. A group renders only the controls
whose ids are in our registry, and a group with no live control does not render
— which is the mechanism `applyCommandRules()` already implements
(`webapp/editor.selection.js:1247`), including its documented carve-out that a
group with *zero* commands has been **collapsed**, not emptied. §11 puts this to
the owner as a named question; it is the one place this note declines the brief
as literally written, and it declines it because the literal reading cannot be
built.

**2. It refuses two contextual tabs.** Picture Format and Shape Format can never
trigger: nothing in this editor can select a picture or a shape. PivotTable
Design is refused for now on a different ground — every one of its controls is a
gap, and a tab with no live control should not ship.

**3. It refuses three category errors, each of which would have filled a gap.**

- `view.theme` (Auto / Light / Dark) is the **application chrome** theme
  (`webapp/editor.paint.js:847`). It is **not** Page Layout ▸ Themes, which is a
  document colour/font/effect set. Page Layout ▸ Themes stays four gaps and
  `view.theme` goes to File ▸ Account ▸ Office Theme, which is Excel's actual
  home for it.
- `data.group` is **outline** grouping; PivotTable Analyze ▸ Group is **pivot
  field** grouping. They share a word and nothing else, so the pivot Group group
  stays three gaps.
- `data.column-stats` is a column profiler and is **not** Home ▸ Analysis ▸
  Analyze Data. It goes to Data ▸ Analysis, where Excel keeps its add-in
  analysis verbs; Analyze Data stays a gap.

**4. It refuses Excel's skin.** No traced, screenshot-derived or
redrawn-from-memory Office glyph. No Excel or Microsoft wordmark, product name
or logo anywhere. **No Excel brand green.** The File tab and the backstage rail
fill with `--oc-accent-color` (`webapp/editor.css:84`), the one host-swappable
token, set by `?accent=` and persisted per user — so every integrator's build
gets *their* brand in that slot. That is both the correct behaviour and the
mitigation (§9). `editor.branding.spec.mjs:57` already asserts that no region of
the chrome names a product, and it is **extended**, not weakened, to refuse
`/Excel|Microsoft|Office/` in the chrome.

**5. It refuses a measurement loop.** §5 replaces `fits()` with a five-step
named ladder. The collapse *point* is measured; the collapse *order* never is.

**6. It refuses lazy tab rendering and selection-dependent commands**, at the
cost of carrying every panel in the DOM. §6.3.

**7. It refuses a second metric set for desktop chrome.**
`webapp/editor.css:2107` records the removal of fourteen desktop-only selectors
and why: they drifted, and two desktop controls ended up *smaller* than the
page's. `.oc-chrome-native` differs in which regions exist, never in their sizes.

**8. It refuses to touch the formula bar, the status bar or the sheet strip.**
Three shipped decisions stand: one flat formula bar with one seam, the selection
summary as text in the status bar, one sheet row rather than two.

**9. It refuses Classic on a coarse pointer, and refuses a ribbon body at all
below 720 px.** §5.3, §5.6.

**10. It refuses to fix four adjacent defects inside this work.** Each is a row,
not a ribbon change: the `embed.d.ts` / `embed.js` `ChromeRegions` mismatch
(§6.4); `toolbar.currency` being drawn in Excel's *Accounting* slot while
writing `$#,##0.00`, which is Currency, with the Accounting mask reachable only
as a combo entry (`webapp/editor.html:414`); `file.export-as-pdf` being matched
by neither `CAPABILITY_COMMANDS` nor `READ_ONLY_SAFE`
(`webapp/editor.core.js:9701`, `:1336`, `:4993`), so `canSaveAs: false` still
gets a PDF out and a viewer — explicitly allowed a copy and a printout — loses
it; and the stale comment at `webapp/editor.core.js:1352` claiming `canShare` is
false in every preset when `:955` and `:958` both set it true.

---

## 9. Risks

**R-1 — command-id churn. Critical.** `commandId()` mints ids by slugifying the
English menu path (`webapp/editor.selection.js:1015`); toolbar ids come from
element ids (`webapp/editor.core.js:11019`). If ids are re-derived from the
ribbon path, **every published id changes at once**, and what breaks is not
cosmetic: `CAPABILITY_COMMANDS` (`webapp/editor.core.js:1336`) and
`READ_ONLY_SAFE` (`webapp/editor.core.js:4993`) both match ids **by regex**, so
a viewer-mode editor whose ids no longer match `/^file\.download/` becomes an
editor that can save. A UI change turning into a security defect, with nothing
red anywhere. *Mitigation:* `UX-RIB-02` freezes the id space into a checked-in
manifest with a static gate **before any ribbon markup is written**, and R2's
proxy model means a ribbon control carries `data-oc-proxy`, never a new
`data-oc-command`. `NATIVE_LABELS` already demonstrates the pattern — it changes
`data-oc-label` and the visible text and never the id
(`webapp/editor.core.js:1206`), proven by
`editor.native-chrome.spec.mjs:566`.

**R-2 — four existing gates degrade to silent vacuous passes rather than
failing. Critical.** A rewrite that only chases red tests lands with four holes
and nothing says so:

| gate | what goes vacuous |
| --- | --- |
| `editor.toolbar-inventory.spec.mjs:26` | sweeps `.tb-collapsed`; with zero such elements the **width budget** — the thing a ribbon is most likely to blow — stops being checked and stays green |
| `editor.chrome-reachability.spec.mjs:47` | sweeps `.toolbar .tb-btn` for clipping; an empty array passes |
| `editor.branding.spec.mjs:64` | reads `#menubar` text with `?? ""`, so the white-label guarantee silently stops covering the replaced region |
| `editor.native-chrome.spec.mjs:735` | `toBeHidden()` passes on a **detached** `#hdr-collapse` |

*Mitigation:* each replacement lands in the **same commit** as the change that
empties the old one, and every new gate carries a vacuity guard the way
`editor.chrome-composition.spec.mjs:284` already does — assert the sweep found
something before asserting what it found.

**R-3 — four assertions are incompatible with a ribbon by construction, and the
failure mode is that somebody edits them. High.**

| assertion | why it cannot hold | retaken as |
| --- | --- | --- |
| `editor.native-chrome.spec.mjs:723` — `.toolbar` height < 49 px | Classic is 93 | per-layout: Simplified ≤ 49, Classic ≤ 100 |
| `editor.native-chrome.spec.mjs:344`, `:378` — desktop grid gain = `.app-header` + `#menubar`, and `{header:0, menubar:0}` | desktop keeps the ribbon and drops only the title strip | new arithmetic: gain = title strip only |
| `editor.native-chrome.spec.mjs:399` — flat 28 px pointer floor on every `.toolbar` control | a ribbon has genuinely mixed density: a 24 px small button stacked three to a group beside a 68 px large split | a **per-control-class** floor, asserted per class: small 24, medium 24, large 40, Simplified 32 |
| `editor.touch-targets.spec.mjs:142` — coarse growth ≤ 16 px across two bands | Excel's touch mode grows the whole ribbon | Simplified 40→48 fits the cap; **Classic is not offered on coarse at all** (§5.6) |

Each encodes a decision made with evidence — the 28 px floor's own comment cites
LibreOffice 26-30, OnlyOffice 32-36 and "~28 is where a mouse target starts
needing aim". This section is the retaking. **Landing a spec edit that relaxes
one of these without the corresponding row above is a review failure**;
`CLAUDE.md` names quietly lowering a promise as the failure mode this project
refuses.

**R-4 — the assignment cost. High.** `docs/88` §8's observation survives its
supersession: a ribbon's real content is the per-tab, per-group, per-size,
per-breakpoint assignment of ~190 commands, authored by hand. Microsoft
publishes neither the Simplified rows nor the classic collapse thresholds. That
is ~60 collapse decisions plus ~190 size/group assignments, against a current
toolbar whose entire authored collapse order is five numbers
(`webapp/editor.html:253`, `:297`, `:315`, `:380`, `:435`). *Mitigation:* the
assignment is **data**, not markup — one manifest consumed by a builder the way
`MENUS` (`webapp/editor.core.js:9611`) is consumed by `buildMenuBar()` — so an
assignment change is a one-line reviewable diff, a typo'd id is a build failure,
and the collapse order can be asserted by name. It gets its own slice and its
own review; **no worker authors assignments and markup in the same change**.

**R-5 — icon supply, and the artwork half of trade dress. High.** The editor
draws inline stroked SVG on a 24-unit viewBox at stroke-width 1.8-1.9,
`fill="none" stroke="currentColor"` (`webapp/editor.html:57`, `:96`, `:196`) — a
Feather/Lucide-shaped set — and **there is no third-party notice file anywhere in
the repository**, so the provenance of the existing 42 glyphs is unrecorded,
which is itself a defect. A ribbon at this structure needs ~190 small plus ~40
large glyphs in one weight on one grid: a 5× increase. **Excel's ribbon glyphs
are Microsoft's copyrighted artwork and are not in the openly-licensed Fluent
subset** [unverified — the licence was not fetched and must be checked before a
single glyph is traced]. *Mitigation:* never trace, screenshot-derive or
redraw-from-memory an Office glyph; pick one permissively-licensed set that
already covers spreadsheet verbs at 16 and 24 px — continuing the existing
Feather/Lucide shape keeps one weight for free — and land a third-party notice
in the same change, which also fixes the existing unrecorded provenance. Icons
run in **parallel from day one**: new files only, no possible conflict, longest
lead time, least ambiguity.

**R-6 — trade dress, the pattern half. Low, and separable from R-5.** The ribbon
*as an interaction model* — tabs, groups, captions, a backstage, KeyTips, a
layout toggle — is not the exposure; LibreOffice and OnlyOffice both ship one.
The exposures are the artwork (R-5), the **brand colour**, and the word "Excel"
in the chrome. There is also an **internal** conflict independent of any legal
question: `editor.branding.spec.mjs:57` asserts no region of the chrome names a
product, and an Excel-green File tab in a white-labelled editor names
Microsoft's. §8.4 is the decision: accent token, never green. §11 puts it to the
owner as a named question, because the default must be the **reversible**
direction and this one is.

**R-7 — content-share cost. Medium, and priced.** §1.2. Simplified is +0;
Classic is −3.7 points against today. Mitigated by making Simplified the default
and Classic opt-in and persisted.

**R-8 — i18n, and silent regression in every host's existing catalogue.
High.** There is no static string catalogue in this repository:
`webapp/editor.i18n.js` is machinery and catalogues are host-supplied through
`setMessages` (the demo German map is `webapp/embed.html:213`). A ribbon adds
~7 tab captions, ~50 group captions and ~190 drawn control labels; **tab and
group captions have no command id at all**, so they do not fit the
`command.<id>` key space and need a third one, `chrome.<slug>`, documented in the
SDK surface in the same change. No `command.<id>` key is renamed — guaranteed
structurally by the proxy model and asserted by a test that the existing German
map still translates every menu item it names. German and Finnish labels run
30-40 % longer than English [unverified as a measured figure; the direction is
not in doubt], and a fixed-height band with an authored width budget has nowhere
to put the overflow — so the gate sets a catalogue whose every string is 1.4× the
English length and asserts no control clips, no group wraps and the band height
is unchanged.

**R-9 — file size and parse cost. Medium.** `webapp/editor.html` is 49 564 bytes
and is the entry document, parsed before anything paints; `webapp/editor.css` is
135 723. Authoring ~190 controls as static markup with inline SVG plausibly
quadruples the parse-blocking document. *Mitigation:* the same manifest-plus-
builder as R-4, so `editor.html` grows by roughly one empty `<div>`; icons become
one inline `<symbol>` sprite referenced by `<use>`, paying for each glyph once
rather than once per instance; ribbon CSS goes in its own file — noting that the
theme-token gate looks only in `webapp/editor.css` and `webapp/style.css`, so
either the gate is extended or the new file defines no tokens. Extend the gate: a
token defined where the gate cannot see it is a token that will be renamed by
accident.

**R-10 — overlay traps, each already sprung once. Medium.** Four, and a ribbon
has far more overlays than a toolbar: overlays parented to `ocOverlayHost` and
not `document.body` (`webapp/editor.core.js:1432`); positioning measured while
the panel is **shown**, because measuring a `hidden` panel yields 0 × 0 and made
every flip and clamp dead code for years (`webapp/editor.core.js:8806`,
`:9979`); the band must **not** be `overflow: hidden` because its flyouts are its
children (`webapp/editor.css:1960`); and a click inside a flyout must not dismiss
it (`webapp/editor.core.js:9430`). *Mitigation:* reuse the existing
`anchorMenu` / `positionSub` helpers rather than writing new ones, and extend
`editor.mobile-menus.spec.mjs` — which already drives real touch events against
every submenu — rather than writing a parallel harness.

**R-11 — the roster has already been folded away twice by chrome changes.
Medium.** `#presence` lives inside `#menubar` specifically so it would not vanish
with the page header (`UX-CHROME-01`), which is why `.oc-hide-menubar` hides
`.menubar > *:not(.presence)` and neutralises the bar's own box
(`webapp/editor.css:267`). Moving it to the title strip is a third opportunity.
*Mitigation:* §6.2's ordering — the roster relocates **first**, through the
existing reversible `chromeHome` mechanism — and
`editor.chrome-regions.spec.mjs:43`'s three assertions stay alive against
whatever region replaces the menu bar.

**R-12 — reverting is not cheap if the ribbon replaces the toolbar in place.
Medium, rising to High at the flip.** `.toolbar` is ~250 lines of markup
(`webapp/editor.html:240-493`) plus its reflow machinery
(`webapp/editor.core.js:9341-9476`) plus ~120 lines of CSS. Under time pressure
that restoration is exactly when the roving-tabindex sweep at `:9453` or the
collapsed-group carve-out at `webapp/editor.selection.js:1265` gets dropped.
*Mitigation:* the ribbon ships **beside** the toolbar behind an allowlisted
`?chrome=ribbon` plus a persisted setting, with the toolbar remaining the default
until the full new gate set is green on main for a stated soak. Every slice up to
the flip is revertible by changing one default.

**R-13 — `browser-smoke` is one serial job of 88 spec files at `workers: 1`, and
this adds ~10 more driving many tabs at many widths. Medium.** *Mitigation:*
budget the new suite's runtime as an explicit number and measure it; prefer a
few gates driving many widths in one page load over many gates that each boot
the editor, since the boot is the expensive part. **And do not retry a flaky
local browser run** — `CLAUDE.md` records that re-running the browser suite eight
times exhausted the machine's ephemeral port range. Push and let CI answer.

**R-14 — two generated documents will lie. Low-Medium.** `docs/47` and `docs/82`
are generated by harnesses CI does not run (they do not match Playwright's
default `testMatch`), and the sweep reads `#tb-percent`, `#tb-numfmt`,
`#tb-currency`, `#tb-fillcolor`, `#tb-size`, `#tb-font-caret`. After a ribbon
those rows turn ❌ on the next regeneration and the map then claims the product
lost features it has. *Mitigation:* re-point the harness selectors in the same
slice that moves the controls, and regenerate once at the flip. Correcting the
generated prose instead is exactly the move this repository refuses.

**R-15 — a gate blind spot directly in this work's path. Medium.** The
ADR-status gate filters tracker rows with `^[A-Z]{2,6}-\d+$` — a **two-segment**
id only — so a three-segment id like `UX-RIB-01` is silently skipped and a
`Done` row citing an ADR left `Proposed` would pass. `check-gate-selftest.py`
forbids a gate defining its own row-id pattern and `tools/tracker_rows.py` exists
because three gates each kept a private answer, yet this one still does, inline.
This programme produces `Done` rows citing `ADR-026`, so it is not hypothetical.
*Mitigation:* it is a row of its own, and it is filed before any `UX-RIB` row
closes. **The `UX-RIB-*` series is used anyway**, because the brief names it and
because a two-segment series would collide with the existing `RIB` namespace
being free-but-unconventional here; the gate is what should move.

---

## 10. The work

Thirteen slices. Rows `UX-RIB-01` … `UX-RIB-13` in
[14](14-EXECUTION-TRACKER.md). Ranked by what unblocks what, not by visible
progress: the first two produce no pixels and everything else is unsafe without
them.

| # | slice | size | depends on | acceptance |
| --- | --- | --- | --- | --- |
| **01** | **Decide it in writing.** This note, `ADR-026`, the `UX-RIB` series, and the four R-3 assertions retaken with numbers | S — 1 day, docs only | — | the repository-policy gates; §12 records which pass today and which cannot from inside this brief |
| **02** | **Freeze the command id space.** The DOM-derived ids become a checked-in manifest; a new static gate asserts manifest == what a booted editor reports; wired into `repository-policy` and into `docs/15`'s table. No visible change | M — 2-3 days; the manifest is mechanical, the review is the work | 01 | the new gate, plus `editor.host-toolbar.spec.mjs:58` and `editor.modes.spec.mjs:56` still green |
| **03** | **Icons.** ~190 small + ~40 large in one licensed set matching the existing stroked style, as one `<symbol>` sprite; a third-party notice file, which also fixes the existing unrecorded provenance | L — longest lead, least ambiguity | 01 | one stroke-width across every ribbon icon; 16 × 16 and 32 × 32 to 0.5 px; no bitmaps; no broken `<use>`; branding gate extended to refuse `/Excel\|Microsoft\|Office/` |
| **04** | **Ribbon shell, Simplified, Home only, behind `?chrome=ribbon`.** Tab strip, body as a fixed band the flex column sees, the manifest-driven builder, the `⋯`, and the full state and motion vocabulary. Toolbar stays default | L — every mechanism is decided here | 02, 03 (stubbed glyphs are fine) | new a11y, states, motion, stability and seam gates; **all 88 existing specs still green**, because the toolbar is untouched |
| **05** | **Classic layout + display-options control.** Three-row groups, captions, hairlines, four sizes, launchers, in-ribbon galleries; the caret; Ctrl+F1; double-click-active-tab; persisted | M | 04 | collapsing gives **exactly** the body's height to `#grid` and expanding gives it back; tabular figures |
| **06** | **The remaining tabs**: Insert, Page Layout, Formulas, Data, Review, View, Help | L — the assignment is the work | 05 | **inventory gate**: every `listCommands()` id becomes *painted* after ≤1 tab activation and ≤1 dropdown, both directions, differences printed; **budget gate**: no group collapsed on **any** tab at 2560/1920/1600/1440/1366/1280, and the 1024 collapse follows the **named** order in sequence |
| **07** | **Backstage.** Full-window, addressable, focus-trapped, grid untouched on exit | L | 05 | covers, traps, returns focus, leaves selection and scroll unchanged; every `file.*` id pointer-reachable inside it; `editor.native-chrome.spec.mjs:818` still green |
| **08** | **QAT + title strip**, and the roster relocation | M | 05, 07 | `editor.chrome-composition.spec.mjs:65` retaken as a **named allowlist**, never deleted; `editor.chrome-regions.spec.mjs:43` re-pointed |
| **09** | **KeyTips**, two levels, per-locale letter table, Escape restoring the origin cell | M-L — keyboard is where advertised-but-dead ships | 06 | stub `runCommand`, fire the sequence, **compare the id**; `editor.excel-shortcuts.spec.mjs:198` still green; `releases_during_edit()` audited |
| **10** | **Contextual tabs**: Table Design, Chart Design, PivotTable Analyze | M | 06 | snapshot `listCommands()` at boot, select a chart, a table and a pivot in turn, assert the sorted array is **identical** each time |
| **11** | **Mobile and touch.** No ribbon body ≤720 px; the per-control-class floor; the restated band-growth budget | M | 06 | `editor.narrow-screens.spec.mjs:27` **unchanged** and green with a mouse **and** under coarse pointer; touch-targets retaken with §9's numbers; `editor.menu-density.spec.mjs` still green |
| **12** | **i18n key spaces** — `chrome.<slug>`, documented in the SDK surface in the same change; the host-catalogue advisory | M | 06, 07 | a 1.4× catalogue clips nothing and changes no band height; the existing German map still translates every menu item it names |
| **13** | **Flip the default.** `.oc-hide-toolbar` extended; `.toolbar` markup and `reflowToolbar`/`overflowToMore` deleted; the two generated harnesses re-pointed and regenerated; the four R-2 replacements confirmed live | M — highest consequence in the programme | 03-12 green on main for a stated soak | the full `browser-smoke` job, with `#tb-status` unmoved, unrenamed and still reading `/^engine v\d/` in `.bottom-bar` |

### The wave rule

Per [67](67-REPOSITORY-REMEDIATION-PLAN.md), items run in parallel only when
they cannot touch the same invariant.

**Forbidden pairs.** (a) **05 with 06** — the same control primitives and the
same width budget; this is the conflict `docs/88` §9 already names, "one worker
in sequence, not two in parallel". (b) **06's tabs internally** — one worker per
tab is tempting and wrong while they share one measurement path and one
stylesheet; they may parallelise only *after* 05 freezes the primitives, and
only if each tab's assignment is its own manifest entry with no shared-file edit.
(c) **07 with 08** — both touch the title strip and both touch
`editor.chrome-composition.spec.mjs:102`. (d) **08 with 09** — both touch the
ribbon keyboard handler and the KeyTip letter space; QAT numerics and tab
alphabetics must be assigned by one worker who can see both. (e) **02 with
anything** — it edits `webapp/editor.selection.js`, which every other slice
depends on; it runs alone.

**Safe pairs.** 03 with 04 (icons are new files only; stub and swap). 10 with 11
(different files, different invariants). 01 with 03 (docs against assets).

`git status --porcelain` after every round; strays have happened. And the
orchestrator verifies by **running** — revert the fix, watch the test go red,
restore, re-run. A worker's report is not evidence.

---

## 11. Questions that are the product owner's

**1. "Inventory parity" cannot be built. Which reading do you want?** §8.1 is
the recommendation — **structure at exact parity, inventory as it grows** — and
the alternative was considered and is worse: ~150 permanently-disabled controls
advertising absence on every screen. If the answer is "draw them anyway", say
so, because it changes slice 06 from an assignment job into an assignment job
plus ~150 disabled-control specifications.

**2. Simplified as the default, with Classic opt-in — confirm.** §4.1's case is
that it is what you asked for ("flatten ribbon"), it is 18 px *cheaper* than
today's chrome where Classic is 34 px dearer, and it is the layout Microsoft
ships on the web. If Classic should be the default instead, the content share
goes from 78 % to 74.3 % on every window, permanently, and that number should be
agreed rather than discovered.

**3. The File tab and the backstage rail: accent token, or Excel green?** The
design says `--oc-accent-color`, so every integrator's build gets their own brand
there. Excel green would name Microsoft's product in a white-labelled editor and
would contradict a shipped assertion (`editor.branding.spec.mjs:57`). There is a
legal component here that is not mine to settle; the **default must be the
reversible direction**, and the accent token is it.

**4. Does the desktop shell keep the ribbon?** Excel's desktop keeps its ribbon
and drops only the title strip. Today's desktop chrome drops the title strip
*and* the menu bar, and an assertion encodes that arithmetic exactly
(`editor.native-chrome.spec.mjs:344`). §9 R-3 retakes it as "gain = title strip
only". Confirm, because it decides how much grid a desktop user gets.

**5. Two Alt letters move.** `Alt+V` (View) becomes `Alt+W`; `Alt+O` (Format)
becomes `Alt+H` (Home). Excel's letters win in this design. Confirm, and it goes
in the release note.

**6. Theme moves from View to File ▸ Account.** That is Excel's only home for it
and the design follows. It also means the shipped assertion that finds the theme
via a "View" menu button must be re-pointed (§3.2). Confirm the move; the
alternative is a second theme control, which
`webapp/editor.html:132` records as the thing that was deliberately avoided.

**7. Where does this sit against the open Excel-parity backlog?** Thirteen
slices is a programme, not a task. `docs/73` holds ~35 unverified findings and
`DEP-08` is the only open P1. The ribbon is P2 work by the tracker's own
severity rubric — nothing reaches the file — and it should be scheduled as such
or explicitly promoted.

---

## 12. Reproducing this

**What was read**, in full or in the cited region:

- `webapp/editor.html` — the toolbar (`:240-493`), the menu bar (`:166-200`),
  the header (`:46-104`), the formula bar (`:495-526`), the bottom bar
  (`:586-657`)
- `webapp/editor.css` — the token blocks (`:70`, `:128`, `:172`), the region
  heights (`:299`, `:352`, `:413`, `:990`, `:1036`), the control states
  (`:763-772`, `:872-877`), the region-hiding rules (`:252-286`), the
  native-chrome block (`:2042-2125`), the reduced-motion clamp (`:1503`), the
  coarse-pointer block (`:2144-2240`)
- `webapp/editor.core.js` — `MENUS` (`:9611-9950`), `reflowToolbar` and the
  collapse machinery (`:9341-9476`), `buildMenuBar` (`:10081-10229`), the
  capability tables (`:1336-1379`), `READ_ONLY_SAFE` (`:4993-5013`), the boot id
  sweep (`:11019`), `anchorMenu` (`:8806`), the key map (`:8256-8660`)
- `webapp/editor.selection.js` — `commandId` (`:1015`), `listCommands`
  (`:1040`), `menuModel` (`:1065-1136`), `runCommand` (`:1146`),
  `applyCommandRules` (`:1166-1286`)
- `webapp/editor.geometry.js:229`, `webapp/editor.i18n.js:41-153`,
  `webapp/editor.drafts.js:209`, `webapp/editor.dialogs.js` (the three context
  menus and the properties dialog), `webapp/embed.js:97-287`,
  `webapp/embed.d.ts:75-82`
- `desktop/src/main.rs:794-940`, `desktop/src/menu.rs`
- `crates/casual-calc-wasm/src/io.rs` — `writable_extensions` (`:267`),
  `session_save_loss_for` (`:349`)
- `docs/88` in full, for its measurements
- the browser suite's chrome specs, by name and line, as cited throughout §9

**What was NOT run.** No build. No `wasm-pack`, no `webapp/serve.py`, no
Playwright, no browser of any kind. **No file in `webapp/`, `crates/`,
`server/` or `desktop/` was modified.** The three files this round wrote are
this note, thirteen rows in [14](14-EXECUTION-TRACKER.md) and one row in
[08](08-ADR-REGISTER.md).

**Consequently every width figure in §5.2 is arithmetic over §2's metrics, not
an observation.** The 1390 px Simplified-Home budget, the 1180 px Classic
figure, and every slack figure in §5.3 are computed. `UX-RIB-05`'s and
`UX-RIB-06`'s gates are how they become measurements, and if the arithmetic is
wrong the first collapse step moves — the *order* does not, because the order is
authored and not derived from the number.

**To reproduce the region-height table in §1.1**, without a browser:

```
grep -n -A3 '^\.app-header\|^\.menubar\|^\.toolbar\|^\.formula-bar\|^\.bottom-bar' webapp/editor.css
```

Each declares its own `height` and its own `border-bottom`/`border-top`; the
figures in §1.1 are those two numbers added, and nothing else contributes,
because every band is `flex: 0 0 auto` in one column
(`webapp/editor.css:228-232`).

**To reproduce the command count**: boot the editor and read
`window.opencalcEditor.listCommands().length`. `webapp/docs.html:353` advertises
189; the assignment in §3 accounts for 188 plus the brand-dependent
`help.about-*`, which is why the two numbers differ by one and why §3.12 says
185 placed plus three named.
