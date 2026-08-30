# 88 — What the editor's chrome should be

## Outcome

The product owner's complaint — *"its looking awkward.. nothing regular
spreadsheet editing desktop tool"* — is the most-repeated one in this project,
and it has been answered three times with metrics: band heights in
`UX-CHROME-05`, a removed branding strip in `UX-DESK-01`, control sizes in both.
It keeps coming back because **it is not a density complaint.** It is a
composition complaint, and the three things that produce it are structural:

1. **A product-branding strip above the menu bar.** 53 px carrying a logo, the
   wordmark *OpenCalc*, an `Alpha` pill, `engine v0.0.0`, a folder button and a
   gear. **None of Excel, LibreOffice Calc, OnlyOffice, Google Sheets or Numbers
   has a region that names the product** — they name the *document*, in the
   window title bar. This region is the single largest structural difference
   from all five.
2. **The selection summary is a frosted floating panel over the grid.** 595 × 33
   px at `bottom: 16px; right: 24px`, `backdrop-filter: blur(14px)`, 11 px
   radius, popover shadow — covering cells, while the status bar 49 px below it
   is empty in the middle. **All four grid competitors put this in the bottom
   strip and none of them floats it.**
3. **Grouping is stated twice and both statements are weak.** Every toolbar
   group is both a filled 10 px-radius capsule (`--oc-surface-color` on a white
   bar — a 3 % luminance step) *and* a 1 px rule. **All four competitors use a
   1 px rule and nothing else**; a filled rounded container behind a group is a
   segmented-control idiom from mobile and web design systems.

Everything else in the complaint follows from a fourth fact, which is a defect
rather than a style:

4. **The toolbar does not fit a 1440 px laptop.** It needs **1463 px** and the
   first group collapses to a `Tools ⌄` chip at **1461 px**; at 1280 px —
   MacBook Air — a third of the toolbar is the three chips `Number ⌄ Data ⌄
   Tools ⌄`. The chips the complaint names are **not a design idiom. They are the
   phone overflow mechanism firing on a desktop**, because the desktop toolbar is
   drawn at touch-target size. The same 33 controls in the desktop metrics need
   **1225 px** and do not collapse until 1223 px.

This note decides the regions, the toolbar composition and its collapse
contract, the formula bar, the bottom strip, and the header bands; it refuses a
ribbon, with the argument on both sides; and it ranks the work by how much of
the complaint each item answers per unit of effort. **Nothing here is
implemented.** §9 is the work, described by content because the rows are the
orchestrator's to file.

---

## 0. How this was measured, and what is *not* measured

Everything about **our** chrome in this note was measured in a running editor:
`wasm-pack` build, `webapp/serve.py`, Playwright driving real Chromium at
device-scale 2, both `editor.html` and `editor.html?chrome=native`. §11
reproduces it. Where a number came from reading source rather than from running,
it says so.

Everything about **the competitors** is documented, and the strength of the
citation varies a great deal by product, which matters when weighing them:

- **LibreOffice Calc and OnlyOffice are exact.** Both publish the layout in
  source — LibreOffice in toolbar/statusbar XML and `.cxx` constants,
  OnlyOffice in `.less` variables and `.template` files — so figures like "the
  formula bar is 22 px" or "the Name Box is 100 px" are literal, not estimates.
- **Excel is estimated.** Microsoft publishes **no** device-independent pixel
  measurement for any chrome region. What Microsoft does publish, and what this
  note leans on, is the *content*: the status-bar option list with defaults, the
  F6 landmark order, the default row height (15 pt = 20 px) and column width
  (8.43 characters = 64 px).
- **Google Sheets is the weakest.** Google publishes nothing and the class names
  are unversioned. Its header-band figures here come from **Luckysheet**, an
  explicit Sheets clone whose defaults are `rowHeaderWidth: 46`,
  `columnHeaderHeight: 20`. Treat them as corroboration, not measurement.

One claim in this note is **derived rather than observed**, and is marked again
where it appears: that the row header clips its label on the last rows of a
sheet. The clip is in the source and the text metric is measured; three attempts
to drive the viewport to row 1,048,576 to photograph it stopped at the data edge
instead.

---

## 1. The complaint, tested

### 1.1 The measurements

Web chrome, 1440 × 900, light, measured:

| region | height | what it carries |
| --- | --- | --- |
| `.app-header` | **53 px** | brand logo, *OpenCalc*, `Alpha` badge, `engine v0.0.0`, Open, Settings |
| `.menubar` | 31 px | File Edit View Insert Format Data Tools Help + header-collapse caret |
| `.toolbar` | 49 px | 33 icon buttons, 2 combo boxes, 8 groups, 7 rules |
| `.formula-bar` | 41 px | Name Box 112 px, `fx`, expand chevron, input |
| **grid** | 685 px | **76 % of the viewport** |
| `.bottom-bar` | 41 px | sheet tabs, `Ready`, `Local only · nothing uploaded`, zoom |

**174 px of chrome above the grid.** The desktop chrome, after `UX-DESK-01`, is
**70 px** (toolbar 37, formula bar 33) and gives the grid 799 px — 89 %. The web
and desktop chromes are two different compositions, not one composition at two
densities, and that is itself part of the problem.

The toolbar width budget — the intrinsic width of the row, measured by forcing
`.toolbar` to 10000 px, spanning first child to last, plus the bar's padding:

| | web chrome | desktop chrome |
| --- | --- | --- |
| visible children | 15 (8 groups, 7 rules) | 15 |
| span, first child to last | 1439 px | 1209 px |
| bar padding | 24 px | 16 px |
| **required width** | **1463 px** | **1225 px** |
| **first group collapses at** | **1461 px** | 1223 px |

**The same 33 controls cost 238 px more in web metrics than in desktop ones.**
Nothing about the inventory differs between the two — only button size, gap and
padding do.

And the collapse sweep, 1920 → 920 px in 40 px steps:

| viewport | chips shown instead of controls |
| --- | --- |
| 1480 + | — |
| **1440** | `Tools` |
| 1400 | `Data` `Tools` |
| **1360–1240** | `Number` `Data` `Tools` |
| 1200–1000 | `Align` `Number` `Data` `Tools` |
| 960 and below | `Font` `Color` `Align` `Number` `Data` `Tools` |

Per-button pitch **inside** a group is 32 px (`tbg-align` is 290 px for 9
children; `tbg-number` 226 px for 7), which is about what Google Sheets spends.
**The waste is not in the buttons.** It is in the **group boundaries**: each of
the seven costs a 10 px bar gap, a 1 px rule carrying `margin: 0 2px`, and
another 10 px bar gap — 25 px — and each of the eight capsules adds 4 px of its
own padding. That is roughly **200 px of the 1463**, spent restating a grouping
that a 1 px rule states on its own.

### 1.2 What makes it read as a web application

Three things, each pointable and each with unanimous evidence against it.

**A product strip.** Excel, LibreOffice and OnlyOffice put the *document* name
in the OS title bar; Google Sheets puts the *document* name in its top row;
Numbers has no in-window row above the toolbar at all. Ours is the only one of
the six that spends a full region naming itself, and it also carries an `Alpha`
badge and `engine v0.0.0` — development state, in the product's chrome.
`UX-CHR-01` already found half of this from a running editor ("a branding strip
is not a toolbar") and `UX-DESK-01` already deleted the region on desktop. What
is left is that the browser still has it.

**Rounded, filled containers.** `.tb-group` is `border-radius: 10px` with a
filled background; `.tb-btn` is `border-radius: 8px`; the sheet strip is a 9 px
capsule holding 7 px tab pills; the Name Box and the formula input are two
independently bordered 8 px boxes; `.sel-stats` is 11 px with a blur. Against
that: OnlyOffice sets `--border-radius-button-toolbar: 1px` and
`--border-radius-dataview-item: 0`; LibreOffice draws flat toolbox items with no
radius; Excel's ribbon controls are square. Material Design 3's own toolbar
guidance says to *"avoid applying rounded corners to the container. This can
imply the container expands or changes upon interaction"* — which is precisely
the misreading a row of filled capsules invites, and precisely the misreading
the complaint reports.

**A frosted panel floating over the content.** There is no analogue anywhere in
the five. Excel puts the summary in the status bar (default Average, Count, Sum);
LibreOffice in status-bar field 10, 200 px wide, default `Average: 3; Sum: 15`;
OnlyOffice in its 25 px status bar with all five statistics on by default; Google
Sheets as a menu button at the bottom-right of the same strip that holds the
sheet tabs. Ours shows six statistics — `Sum · Avg · Min · Max · Numbers ·
Count` — in a blurred panel over the cells, with the status bar's middle empty.

### 1.3 Two parts of the brief's diagnosis the measurements refute

Stated because acting on either would waste work.

**"Our toolbar is a row of small icon pills with dropdown chips (`Data ▾`,
`Tools ▾`)" — the chips are not a chosen idiom.** They are `.tb-collapsed`
buttons, authored `hidden`, shown only by `reflowToolbar()` when the row
overflows. At 1920 px there are none. They appear because the bar needs 1463 px.
Restyling them is the wrong fix; making them not appear on a desktop is the right
one, and that is a width budget (§3.4), not a visual treatment.

**"Most put the Name Box immediately left of the formula input with an `fx`
affordance" — we already do, and our Name Box width is correct.** Ours is
112 px; Excel's is ~110–130 px and user-resizable, OnlyOffice's is exactly
100 px, LibreOffice's is ~18 characters and deliberately width-matched to the
Font Name combo. Ours is in range and needs no change. What *is* wrong in that
region is different and is §4: the two controls are separately bordered boxes
rather than one bar with a seam, and the expand chevron is in a position no
competitor uses.

---

## 2. The regions

**Five regions, and the branding strip is not one of them.**

| # | region | web | desktop | embedded | height |
| --- | --- | --- | --- | --- | --- |
| 1 | **Menu bar** | HTML | **OS draws it** | HTML unless the host asserts its own | 28 px |
| 2 | **Toolbar** | ✓ | ✓ | ✓ (host may hide) | 32 px |
| 3 | **Formula bar** | ✓ | ✓ | ✓ (host may hide) | 26 px + expansion |
| 4 | **Grid** — header bands, canvas, scrollbars | ✓ | ✓ | ✓ | rest |
| 5 | **Bottom strip** — tabs left, status right | ✓ | ✓ | ✓ (host may hide) | 28 px |

**Why each is where it is.**

The **menu bar is first because it is the complete command surface** and
everything below it is a shortcut into it. Apple's HIG states the rule this
project should adopt verbatim: *"Make every toolbar item available as a command
in the menu bar. Because people can customize the toolbar or hide it, it can't be
the only place that presents a command."* Our toolbar is already hideable by a
host (`.oc-hide-toolbar`) and already collapses groups into flyouts, so the rule
is not hypothetical — it is what makes both of those safe.

The **toolbar is second because it is the frequency shortcut**, and it is one row
because the vertical budget is the resource this complaint is about (§8).

The **formula bar sits between the toolbar and the grid** in every one of the
five that has one, because it belongs to the *cell*, not to the document: it is
the last thing before the data and the first thing after it. Numbers is the
exception and §8 explains why copying it would be a mistake.

The **bottom strip is one row, not two.** Excel and LibreOffice use two (tabs,
then status); Google Sheets and OnlyOffice use one. One is right here because we
already have one, because OnlyOffice proves a 25 px row holds tab navigation,
tabs, save state, the selection summary and zoom, and because in a browser tab
every reclaimed row is grid.

**The branding strip is deleted, in every chrome**, and its four occupants have
homes that already exist:

| occupant | goes to | on whose authority |
| --- | --- | --- |
| logo, *OpenCalc*, `Alpha` | the landing page; the editor's `<title>` and favicon already identify it | no competitor names its product in the editor |
| `engine v0.0.0` / `#tb-status` | status bar | `UX-DESK-01` already does this on desktop |
| Open (folder icon) | File menu only | `UX-CHR-01`: *"a branding strip is not a toolbar"* |
| Settings (gear) | Tools menu; Theme to View | `UX-CHR-01` |

That reclaims **53 px, +8 % of grid height at 900 px** — and, more importantly,
**makes the web and desktop region stacks identical.** After this, the three
chromes differ in exactly one structural way (who draws the menu bar) rather than
in two compositions.

---

## 3. Toolbar composition

### 3.1 Grouping: a 1 px rule, and nothing else

Delete the capsule. `.tb-group`'s `background`, `border-radius: 10px` and
`padding: 2px` go; the `.tb-sep` rule stays and becomes the only statement of
grouping. Evidence is unanimous and comes from source in two of the four cases:
LibreOffice declares `<toolbar:toolbarseparator/>` as a first-class element in
`standardbar.xml` and `formatobjectbar.xml`; OnlyOffice renders
`separator.short`/`.long` as `border-left: 1px solid @border-toolbar`; Excel uses
whitespace, a group caption and a thin rule; Sheets uses hairline dividers.

Group boundaries also stop costing 21 px. **6 px rule 6 px** is the budget.

### 3.2 Shape and size

| | now (web) | now (desktop) | decided |
| --- | --- | --- | --- |
| button | 30 × 30, radius 8 | 26 × 26, radius 6 | **26 × 26, radius 3**, both |
| icon | 16 px | 16 px | 16 px, unchanged |
| bar gap | 10 px | 6 px | **6 px**, both |
| bar padding | 12 px | 8 px | **8 px**, both |
| band | 49 px | 37 px | **32 px**, both |

The 16 px icon does not shrink: LibreOffice's *small* icon size is 16 and
shrinking the glyph would trade a dense toolbar for an unreadable one — the
argument the existing `.oc-chrome-native` comment already makes, kept.

The single metric set is the point. Two metric sets exist today only because the
web one is too loose; once it is right there is nothing for the desktop one to
correct.

### 3.3 What is labelled

**Text only where the value is the state.** This is the rule all four follow and
it explains the apparent contradiction between Nielsen Norman's *"a text label
must be present alongside an icon"* and the fact that spreadsheet toolbars are
mostly icon-only: the spreadsheet icon vocabulary is one of the few genuinely
learned ones. Jensen Harris, who ran the Office 2007 redesign, states both halves
— label everything *except* "those items which work just as well as unlabeled
icons (Bold, Italic, Center, etc.)".

So: **inherit the learned icons, and never invent a new icon-only one.**

Labelled, showing a value:

- **Font name** — combo box with a visible text field. Already correct.
- **Font size** — same. Already correct.
- **Number format** — **new.** A combo box reading `General` / `Number` /
  `Currency` / `Accounting` / `Percentage` / `Date` / `Time` / `Scientific` /
  `Text` / `Custom`. Excel has one in the Number group; OnlyOffice has one *and*
  the quick buttons; Sheets shows `123 ⌄`. Only LibreOffice omits it. It replaces
  the `#` icon, costs about 96 px, and answers a question our chrome **cannot
  currently answer at all** — a user looking at `43.48` has no way to find out
  whether that cell is General or a two-decimal number without opening a dialog.

Everything else stays icon-only. The dropdown caret becomes a **sub-glyph inside
the button**, one hit target — the way Sheets, LibreOffice's `ToolBoxItemBits::
DROPDOWN` and Excel's split buttons all do it — not a companion chip.

### 3.4 The width contract, and the inventory it forces

**The toolbar must fit 1280 px with no group collapsed.** That is the acceptance
criterion, it is checkable, and it is the thing that makes the chips stop
appearing on a desktop.

From 1463 px:

| change | saves |
| --- | --- |
| button 30 → 26 px (× 33) | −132 px |
| bar gap 10 → 6 px (× 14) | −56 px |
| bar padding 12 → 8 px | −8 px |
| capsule padding removed (× 8 groups) | −32 px |
| `.tb-sep`'s `margin: 0 2px` folded into the gap (× 7) | −28 px |
| **metrics subtotal** | **≈ 1207 px** |
| **number-format combo added** | **+96 px** |
| **subtotal** | **≈ 1303 px** |

Note the metrics subtotal lands within 18 px of the desktop chrome's measured
1225 px, which is the check on this arithmetic: it is predicting a bar we have
already built and measured.

Still 23 px over 1280, which forces the honest conclusion: **metrics alone do
not buy the number-format readout.** One group must leave the bar. `tbg-tools` —
data validation, conditional formatting, comments — is 98 px plus its gap and
rule (113 px in total), is the lowest-priority group already
(`data-collapse="1"`), all three commands are already in the menus, and **Google
Sheets carries none of the three on its toolbar either.** Removing it lands the
bar at **≈ 1190 px**, inside 1280 with 90 px of headroom for a theme's own
variation and for the next control that has to go somewhere.

That is a decision about **inventory**, and it is the one this note most wants on
the record: a single-row toolbar is a fixed budget, and the way to keep it is to
govern what goes in it, not to let a collapse mechanism absorb the overspend
invisibly.

### 3.5 Collapse order is authored, not computed

This is the sharpest finding in the note. `.toolbar`'s comment describes
`reflowToolbar()` as *"Excel-ribbon-style progressive collapse"*. It is the
opposite of what the ribbon does. Harris, on the ribbon's scaling, is explicit:
*"the order in which the chunks collapse into different versions is also designed
by us and not by the computer. There's no attempt to 'auto-scale' the UI."* The
four rules that go with it, and how ours fares:

| Office's rule | ours |
| --- | --- |
| No commands appear or disappear between sizes | **broken** — a collapsed group's controls leave the bar |
| Multiple commands may not roll up into a menu at smaller sizes | **broken** — a whole group becomes one `Label ⌄` menu |
| Keep labels as long as possible unless icons are well-known | not applicable — there are no labels to keep |
| A popup chunk shows the layout it would have had inline | **broken deliberately** — the flyout is a vertical labelled list |

Three of four broken is not an argument for reversing them, and this note does
**not** reverse the fourth. The flyout's vertical labelled list is right, and the
CSS comment beside it gives the correct reason: once a group has collapsed,
unlabelled glyphs in a floating box are a guessing game. Harris's rule assumes
the ribbon's popup carries the same labels the ribbon had; ours has none on the
bar, so matching the bar's layout would be three mystery glyphs.

The resolution is that **the flyout is a phone affordance, and the fix is that it
should almost never fire on a desktop.** With §3.4's budget it fires below
1280 px, where a labelled list is the only readable answer. The collapse *order*
stays authored — Tools → Data → Number → Align → Color → Font — which is what
`data-collapse` already encodes; what changes is that a desktop user never sees
it. The `⋯` overflow of last resort stays for phones, where `UX-MOB-01` put it
and where `docs/12` §4.5 confirms it keeps every control reachable at 390 px.

---

## 4. The formula bar

**Height 26 px**, between LibreOffice's hard-coded 22 and OnlyOffice's 20 px
minimum, and down from 41.

**Order, left to right:** Name Box → **1 px seam** → `fx` → input → *(spacer)* →
**expand chevron at the far right edge**.

Four decisions.

**Keep the Name Box at 112 px, and stop drawing it as a box.** The width is
right (§1.3). What is wrong is that it and the input are two independently
bordered, 8 px-radius controls floating on a white bar with 63 px of buttons
between them — an HTML form. All four competitors draw **one flat bar with a
single 1 px seam** after the Name Box: OnlyOffice literally
`--celleditor-cell-name-border-right: 1px solid var(--border-toolbar)`;
LibreOffice `InsertSeparator(1)` immediately after the Name Box widget; Sheets a
vertical divider; Excel a divider that doubles as the resize grip. The formula
input in all four has **no border of its own** and sits directly on the bar. This
is the single clearest "web form" tell in the region and it is a CSS change.

**Move the expand chevron to the far right.** Unanimous: Excel's is at the far
right end (`Ctrl+Shift+U`), LibreOffice's is two stacked chevrons at the far
right, OnlyOffice's `#ce-btn-expand` is `float: right`, Sheets' is a chevron at
the right end. Ours is at **x = 160**, immediately left of the input and 26 px
from the Name Box's own caret, where it reads as a second dropdown. Measured at
1280 px: the expand control is at 160 and the bar's right edge is at 1280 with
**nothing there at all**. `Ctrl+Shift+U` is already bound and already matches
Excel — the shortcut is right and the control is in the wrong place.

**`fx` stays a button.** Excel's opens Insert Function; LibreOffice's opens the
Function Wizard. Sheets' `fx` is decorative. Ours is a button; keep it.

**The expansion must actually show more than one line.** Observed: with a long
formula in `D5` and the bar expanded, `#formula-input` is an **86 px-tall box
containing one vertically-centred line**, the bar grows to 99 px, and 58 px of
grid is surrendered for no gain. The cause is in the markup — `#formula-input` is
`<input type="text">`, and an `<input>` never wraps regardless of the
`white-space: normal` it computes. OnlyOffice's cell editor is a `<textarea>` for
exactly this reason, and the inline cell editor in this same product is already a
`<textarea>` with the comment explaining why ("Alt+Enter puts a hard line break
in a cell, which an `<input>` cannot hold"). The formula bar needs the same
element for the same reason. Expanded height should then be remembered and
drag-resizable from the bar's bottom edge, as Excel and LibreOffice both allow —
that part is second-order.

---

## 5. The sheet-tab strip and the status bar

One strip, 28 px, laid out:

```
◀ ▶  +  ☰ │ [ Sheet1 ][ Sheet2 ][ … ]  …  Ready   Average: 3   Count: 5   Sum: 15   │  − ──○── +  100%
└── pinned ──┘└──── scrolls ────┘                └──── selection summary ────┘   └── zoom ──┘
```

**The navigation controls are pinned, outside the scroller, on the left.**
Verified defect, in a running editor: with 12 sheets at 1280 px the strip
overflows (897 px of content in 796 px) and **both the `+` and the `☰`
all-sheets menu scroll out of view**, because `renderTabs()` appends them into
`#sheet-tabs`, which is the `overflow-x: auto` element. There are no navigation
arrows at all. Every competitor pins these: LibreOffice has five square buttons
`|◀ ◀ ▶ ▶| ⊕` all on the left; OnlyOffice has `◀ ▶` then `⊕` then a sheet-list
dropdown, all `float: left` outside the absolutely-positioned tab box; Sheets has
`+` then `☰ All sheets` to the left of the first tab; Excel puts its scroll
arrows at the far left and its All Sheets menu bottom-left. Three of four put
them on the **left**, which is also the only placement that cannot be pushed off
by tabs.

**The selection summary moves off the grid and into this strip**, and drops from
six statistics to three.

- **Position:** right of centre, before zoom. Excel, LibreOffice and OnlyOffice
  all put it there; Sheets puts it at the far right of the same strip.
- **Default set: Average, Count, Sum** — Excel's default, in Excel's order, with
  Excel's wording `Average: 3   Count: 5   Sum: 15`. Ours currently shows
  `Sum · Avg · Min · Max · Numbers · Count`, which is more than any competitor's
  default except OnlyOffice's five, and it is six values a user did not ask for
  in a panel over their data.
- **Min, Max and Numbers stay available**, from a right-click on the status bar —
  Excel's mechanism, and the one that makes a three-item default safe.
- **Empty when there is nothing to summarise.** Already true (`updateStats()`
  clears on a single-cell selection); the strip should give back the space, as
  AG Grid's status bar does and as Sheets' summary does.
- **Delete the treatment**: `backdrop-filter: blur(14px)`, 11 px radius and the
  popover shadow go. It becomes text in a bar.

**`Ready` stays at the left of the status region**, which is where Excel's Cell
Mode indicator sits — the one place our bottom bar is already right.

**`Local only · nothing uploaded` is a web page's reassurance about the
network.** It is already hidden in desktop chrome and should be hidden in
embedded chrome too (§7).

---

## 6. Row/column headers and the select-all corner

Currently `HEADER_H = 24` and `HEADER_W = 46`, both hard constants.

| | column band | row band | derived from row digits? |
| --- | --- | --- | --- |
| Excel | ~20 px (est.) | ~26–30 px (est.) | **yes** |
| LibreOffice | text height + 3 ≈ 17–19 px | bold width of `"8888"` + 8 ≈ 34–40 px | **yes**, stepped |
| OnlyOffice | ~19–20 px | ~28–34 px | **yes** |
| Sheets / Luckysheet | 20 px | 46 px | no |
| **ours** | **24 px** | **46 px** | **no** |

**Column band 24 → 20 px.** Ours is the tallest of the five, and 20 px is where
three of the four sit; it is also exactly Excel's default row height, which is
what makes the band read as "one row tall" rather than as a bar.

**Row band 46 px → derived, stepped, LibreOffice's rule.** A fixed 46 px is wrong
at both ends. At rows 1–99 the label needs 6–16 px and the band spends 46, so
roughly 30 px of grid width is given away on every frame of every ordinary sheet.
At the bottom of the sheet it is too narrow: measured at the header's own font
(`ctx.font = "12px system-ui, sans-serif"`, `editor.core.js`), `1048576` is
**51 px** wide, and the row header is clipped to `HW - GW` = 46 px — so the last
rows draw a truncated number. **This one is derived, not observed**: the clip and
the font are in the source and the 51 px is measured, but three attempts to drive
the viewport to row 1,048,576 stopped at the data edge. It should be confirmed
before it is fixed. LibreOffice's rule — size the band from the bold width of
`"8888"` + 8 px, stepping up at 5, 6 and 7 digits — is the simplest correct one
and is already stepped so it does not reflow while scrolling.

**Draw a select-all mark in the corner.** Ours currently contains **two
freeze-drag handles and nothing that says "select all"** — a white rounded pill
above a grey bar, which reads as debris. Excel draws a right-angled triangle in
the lower-right of the corner box; Sheets a grey box with a triangle; OnlyOffice
a triangle. LibreOffice draws nothing at all, but LibreOffice's corner is a
vestigial bevel — two 1 px shadow lines on the bottom and right edges — and is
the outlier of the four. Draw the triangle; keep the freeze handles but reveal
them on hover of the corner rather than permanently.

**Make the band read as a band.** `headerBg` is `--oc-surface-color` (`#f6f7f9`)
against a grid background of `#ffffff` — a 3 % luminance step, which is why the
headers in every screenshot look like part of the grid with a hairline through
them. Excel uses a distinct light grey, deliberately different from the ribbon
background; LibreOffice uses the system face colour so it tracks the desktop
theme. The step needs to be large enough to see, and this interacts with the
gridline-contrast work `UX-A11Y-01` already did — it should be checked against
the same measurements rather than picked by eye.

---

## 7. Web, desktop and embedded

After §2, **one composition, three mounts**, differing in exactly one structural
way.

**Who draws the menu bar.** On desktop the OS does — `TAURI-005` builds it on
all three platforms, and `.oc-chrome-native #menubar { display: none }` hides
ours. That is the evidence `UX-CHROME-01` demands: **hiding a menu bar is only
allowed when another one demonstrably exists.** The rule was learned expensively
— the roster lived inside the menu bar, so hiding the bar took presence with it,
and a host that hid the menus lost the ability to see who else was in the
document. Two consequences that survive into this design:

- A host that asks for `?hide=menubar` in an embed is asserting it has its own
  command surface. It is the host's assertion to make, and the editor should keep
  hiding *items* rather than the bar itself, so the roster survives — which is
  what the current `.oc-hide-menubar .menubar > *:not(.presence)` rule already
  does and should keep doing.
- Every toolbar command must exist in the menus (§2), or hiding the toolbar loses
  capability rather than convenience. `UX-DESK-05` is the open question in the
  other direction — whether "listed in `listCommands()`" is meant to imply
  "reachable by pointer" — and this note does not settle it, but §2's rule makes
  the *menu* the invariant surface, which is the answer that makes `UX-DESK-05`'s
  choice cheap either way.

**What no longer differs.** Metrics (§3.2 makes them one set) and the branding
strip (§2 deletes it everywhere). The desktop-only relocations `UX-DESK-01`
built — `#tb-status` and `#presence` into the status bar — become the default
everywhere rather than a mode, which also removes the ordering hazard
`UX-DESK-04` found, where `wirePresence()` re-homes `#presence` after the mode
has already placed it.

**What differs by policy, not composition.** `Local only · nothing uploaded` is a
browser-tab reassurance; it stays hidden on desktop and should also be hidden in
embedded chrome, where the host owns the storage story. Embedded chrome keeps
every existing `?hide=` region switch.

---

## 8. What this refuses

### 8.1 A ribbon — argued both ways

**The case for.** It is the honest one and it is not weak: our toolbar cannot
hold its inventory. We are already collapsing groups at 1461 px, §3.4 has to
*remove* a group to fit 1280 px, and what a ribbon buys is exactly the thing we
are short of — room, plus authored labels, plus a place to put the next fifty
commands without another negotiation. Excel and OnlyOffice both have one. If the
goal is inventory parity with Excel, a single 32 px row will lose, and it will
lose repeatedly and visibly.

**The case against, which wins here, on four grounds.**

*It costs the resource the complaint is about.* OnlyOffice's tab strip plus
controls row is **32 + 66 = 98 px**, exact from `variables.less` and
`colors-table.less`, plus an editor header above it. Excel's tab strip plus
commands is ~96–105 px estimated. The stack this note decides is **28 + 32 + 26 =
86 px including a menu bar**. `docs/12` §4.4 already measures our 76 % content
share as an advantage over Sheets; a ribbon spends it.

*The cost is not the drawing, it is the assignment.* A ribbon is a tab model, and
its real content is the decision about which of ~197 commands goes on which tab,
in which group, at which size, with which collapse variant — authored by hand for
every breakpoint, per Harris. The menus would still have to exist alongside it.
That is a whole-editor redesign, and `docs/86` §6.2 already refused it once on
exactly this ground: it is owned by `docs/12` and `docs/47`, and it is not a
desktop question.

*The strongest counter-example is on the other side.* **LibreOffice ships a
ribbon — its "Tabbed" interface variant — and does not default to it.** The
default is Standard: a menu bar and two single-row toolbars. That is the best
available evidence that a ribbon is not required in order to read as a desktop
spreadsheet application.

*It does not answer the complaint.* Every finding in §1 is a placement or a
metric. A ribbon changes none of them: a branding strip above a ribbon is still a
branding strip, a frosted panel over the grid is still frosted, and capsuled
groups inside a ribbon still read as segmented controls.

**And if the inventory does outgrow one row**, the cheaper move is LibreOffice's:
a second, optional, individually-hideable toolbar row — not a tab model. That is
the escape hatch, and naming it is how this refusal stays honest.

### 8.2 The rest

**Numbers' model** — no persistent formula bar, sheet tabs at the top, tables as
canvas objects. It is coherent inside Numbers and incompatible with ours: there
is no canonical `A1` for a sheet, no sheet-wide used range, no whole-column
reference, and nowhere to hang a formula bar, because "the active cell" is
ambiguous until a table is picked. Worth taking from Numbers: exactly one thing,
its floating formula editor's *geometry* — movable, resizable from any edge — as
a model for §4's expansion. Note also that Numbers **removed** a formula bar it
once had; that is a reversal specific to its object model, not evidence a formula
bar is optional.

**A custom title bar** — already refused by `docs/86` §6.2, and the document name
is already in the OS title bar via `desktop/src/title.rs`.

**Removing the HTML menu bar on the web.** It is the complete command surface
(§2) and `UX-CHROME-01`'s rule forbids hiding it without another one.

**A command palette.** `docs/12` §4.2 calls it the cheapest discoverability win
available and it probably is — but it is a command surface, not chrome
composition, and folding it in here would make this note about something else.

**Restyling the collapse chips.** §1.3: the fix is that they should not appear.

---

## 9. The work, ranked by complaint answered per unit of work

Described by content. **No tracker ids are proposed here** — the rows are the
orchestrator's to file.

| # | what | answers | work | depends on |
| --- | --- | --- | --- | --- |
| **1** | **Delete the branding strip in every chrome**; move its four occupants to the menus and the status bar (§2) | the largest structural difference from all five competitors; +53 px of grid; makes web and desktop one composition | **small** — `UX-DESK-01` already built and tested the desktop half; this generalises a done thing | `UX-CHR-01` first, since it decides where Theme and Open go |
| **2** | **Selection summary into the status bar**, default Average/Count/Sum, no blur, no float (§5) | removes the most conspicuously web-app element; stops covering cells; fills the visibly empty status bar | **small** — one element moves, one CSS block is deleted | none |
| **3** | **Toolbar shape and metrics**: no capsules, 1 px rules, 26 px buttons, 3 px radius, 6 px gaps, one metric set (§3.1–3.2) | makes it read as a toolbar rather than a segmented control; removes most of the chip range | **small–medium** — mostly CSS; the `.oc-chrome-native` metric block collapses into the base | none; enables 4 |
| **4** | **Govern the inventory to fit 1280 px uncollapsed**, and add the number-format readout (§3.3–3.4) | the chips stop appearing at mainstream widths; answers "what format is this cell" for the first time | **medium** — the combo is a new control wired to existing number-format commands | 3, for the width budget |
| **5** | **Pin `+`, `☰` and tab-scroll outside the scroller, on the left** (§5) | a verified reachability defect: both controls are unreachable at 12 sheets | **small** — a markup move in `renderTabs()` | none |
| **6** | **Formula bar: one flat bar with a 1 px seam, expand chevron to the far right, a control that can show two lines** (§4) | the clearest "HTML form" tell in the chrome; and the expand button currently costs 58 px of grid to show one line | **medium**, and the riskiest — the tint mirror positions against `.formula-bar`, and `<input>`→`<textarea>` touches the editing path | none, but sequence it alone |
| **7** | **Header bands: 20 px column, derived row width, a select-all triangle, a visible band step** (§6) | makes the grid itself look native; fixes a clipped row label | **medium** — canvas draw path plus `HEADER_W`/`HEADER_H` | confirm the clip first (§0) |

**Order:** 1, 2, 3 first — they are the three cheapest and they are three of the
four findings in §1. Then 5 (independent, small, fixes a real defect), then 4
(needs 3), then 7, then 6 alone.

**Two of these must not run concurrently.** 3 and 4 are the same file and the
same width budget; 6 and 2 both touch `.bottom-bar`/`.formula-bar` CSS. Per
`docs/67`'s wave rule, that is one worker in sequence, not two in parallel.

---

## 10. Questions that are the product owner's

**1. Is `editor.html` a product, or a demo of a component?** §2 deletes the
branding strip on the evidence that no competitor names its product in the
editor. That is unambiguous if the page is a *demo* — the branding belongs on
`index.html`. If it is meant to be a *product* that people use in a browser, then
something has to carry document identity, save state and Share, and that is a
**document** strip (name, dirty marker, Share, presence) rather than a **product**
strip (logo, wordmark, version badge). The two look similar and are not the same
region. This changes §2 and it is not mine to decide.

**2. Excel's selection summary or Sheets'?** Excel shows three statistics at once
and adds more by right-click; Sheets shows one at a time as a menu button you
click to switch. §5 recommends Excel's, because an Excel migrant expects it and
because `docs/12` ranks arriving-from-Excel as the case that matters. Sheets' is
narrower and survives a small window better. This is a preference, not a
measurement.

**3. Does "a viable alternative to Excel" mean matching Excel's toolbar
inventory, or its composition?** §3.4 assumes **composition**, and governs the
inventory down — it removes a group to make room for a readout. If inventory
parity is the goal, §8.1's refusal of a ribbon should be reopened, because a
single row will not hold it and the escape hatch (a second optional toolbar row)
buys one row, not five.

---

## 11. Reproducing this

Every measurement of our own chrome, from a clean tree:

```sh
wasm-pack build crates/casual-calc-wasm --target web --out-dir "$PWD/webapp/pkg"
cp fixtures/generated/minimal.xlsx webapp/sample.xlsx
python3 webapp/serve.py 8099
```

Then, against `http://localhost:8099/editor.html`:

- **Region heights** (§1.1) — `getBoundingClientRect()` on `.app-header`,
  `.menubar`, `.toolbar`, `.formula-bar`, `#grid`, `.bottom-bar`, at 1440 × 900,
  and again with `?chrome=native`.
- **Toolbar width budget** (§1.1) — set `.toolbar`'s `style.width` to `10000px`
  so nothing can collapse, then measure **first visible child's `left` to last
  visible child's `right`**, plus the bar's horizontal padding.
  **Do not sum child widths and add `(n − 1) × gap`**: that undercounts by 28 px
  because `.tb-sep` carries `margin: 0 2px` on top of the flex gap, and the
  first draft of this note reported 1435 px for that reason. The check that
  catches it is that the sum must agree with the width at which
  `reflowToolbar()` fires — 1463 required against 1461 observed, not 1435
  against 1461.
- **Collapse sweep** (§1.1) — step the viewport down one pixel at a time across
  the suspected threshold and read
  `[...document.querySelectorAll(".tb-collapsed")].filter(e => !e.hidden)`.
  A 40 px step finds the range; only a 1 px step finds the number.
- **Sheet-strip overflow** (§5) — click `.sheet-add` eleven times at 1280 px,
  then compare each of `.sheet-add` and `.sheet-all`'s
  `getBoundingClientRect()` against `#sheet-tabs`'s.
- **Formula-bar expansion** (§4) — type a long formula, click `#fx-expand`, and
  read `#formula-input`'s `tagName`, box height and `scrollWidth` vs
  `clientWidth`.
- **Row-label width** (§6) — `measureText("1048576")` at
  `12px system-ui, sans-serif`, against `HEADER_W` in `webapp/editor.core.js`.

The chrome constants themselves are `HEADER_W`/`HEADER_H` in
`webapp/editor.core.js`, the region rules in `webapp/editor.css`, and the
collapse machinery in `reflowToolbar()` in `webapp/editor.core.js`.

## References

- [12](12-COMPETITIVE-ANALYSIS.md) — density and content share (§4.4),
  discoverability (§4.2), the first five minutes (§4.7), mobile (§4.5). Note
  `DOC-051`: several of its claims are stale, and this note re-measured rather
  than cited where they overlap.
- [47](47-UX-AND-FEATURE-MAP.md) — the ranked daily misses this chrome serves.
- [49](49-DESIGN-SYSTEM.md) — the token surface these metrics are expressed in.
  Its Part A inventory is stale against the current CSS and was not relied on.
- [67](67-REPOSITORY-REMEDIATION-PLAN.md) — the wave rule §9 applies to
  sequencing.
- [73](73-EXCEL-UX-PARITY-AUDIT.md) — what an Excel user notices, ranked.
- [82](82-UX-VISUAL-AUDIT.md) — the generated geometry audit; this note is the
  composition question that audit cannot ask.
- [86](86-DESKTOP-RELEASE-IDENTITY-SETTINGS-AND-UPDATES.md) §6 — what
  `UX-DESK-01` left of the window, and the first refusal of a ribbon.
- [87](87-THE-DESKTOP-DIMENSION.md) — what a desktop office application is
  expected to *be*; this note is what it should look like.
