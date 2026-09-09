# 93 — Google Sheets' chrome, as this editor's second

**Nothing in this note is implemented. No code was changed to write it.** Every
`file:line` below was opened and read in this round; every number about *our*
chrome is the declared value in the stylesheet or a sum of them, and says so.
§18 reproduces it. §15 is the work, sliced; §16 is the tracker rows, with ids
left `<assign>`.

This is the **second of two chromes**, and it is the pair of
[91](91-EXCEL-RIBBON-CHROME-DESIGN.md). Both ship; the user picks (§12). `91`
specifies the ribbon and `ADR-026` governs its command-id policy; this note
specifies the Sheets-shaped chrome and inherits that policy unchanged. Where the
two notes disagree on a shared value, §14 R-7 names the disagreement and this
note does not resolve it unilaterally.

[88](88-EDITOR-CHROME-COMPOSITION.md) is used here **as a measurement source
only** — its region heights, its 1463 px width budget and 1461 px collapse
point, its group-boundary arithmetic, its sheet-strip finding, its formula-bar
composition. Its *conclusions* are superseded by the product owner's ruling that
the editor gets both interfaces. This note does not re-argue the decision. It
specifies the thing.

---

## 1. Outcome

Afterwards a person can pick the chrome they want and get a genuinely different
editor rather than a reskin, and the one specified here is the one Google Sheets
users already know: a menu bar, one toolbar row, panels instead of dialogs, and
collaboration drawn in the chrome rather than hidden behind a menu. Concretely,
nine things become true that are not true today.

1. **The chrome is the smallest of the three.** 140 px above the grid against
   today's 162, the ribbon's 144 Simplified and its 196 Classic (§3). One
   keystroke (`Ctrl+F1`) takes it to 76.
2. **Collaboration is visible before there is anyone to collaborate with.**
   `#presence` is `hidden` until `collabSession` is truthy
   (`webapp/editor.presence.js:260-263`) and `Share…` is the tenth item of the
   File menu (`webapp/editor.core.js:9679`). This project runs a clustered OT
   server with presence, resume and relay. The chrome does not say so. §4 puts
   the avatars, the Share pill and one honest save state in the top strip, where
   they **populate rather than appear**.
3. **There is one claim about whether your work is safe, instead of three that
   disagree.** `#doc-state` says "Saved"/"Unsaved changes" from `isDirty()` on a
   250 ms poll (`webapp/editor.core.js:9188`, `:9193`); `#autosave-state` is
   hidden whenever the draft autosave is working (`webapp/editor.drafts.js:514-521`);
   `.bottom-status` permanently reads "Local only · nothing uploaded"
   (`webapp/editor.html:609`). A user with a healthy draft written seconds ago
   can be reading two of those at once, and the one that reflects what actually
   protects them is the one that is invisible when it works.
4. **Nine blocking modals become panels**, so a rule is written against the data
   it is about rather than from memory (§7).
5. **Four `docs/92` §5 rows — engine capability with no route to it — get a
   home in the same act.** Chart grouping (R-3), the four missing conditional-
   format operators (R-8), shrink-to-fit (R-14) and sort beyond three keys
   (C-8). Two of those are cases where the file can already carry something the
   UI cannot author, so a workbook opened and edited here **silently narrows**.
6. **Every command you cannot see is one keystroke away.** `Alt+/` opens a tool
   finder over `menuModel()` (`webapp/editor.selection.js:1065`), which already
   carries every leaf's id, label, enabled state and menu path derived from the
   live DOM. That is what buys the right to a 30-control toolbar; without it,
   restraint is amputation.
7. **~60 verbs that live only in context menus get command ids.** Paste Special
   and its variants (`webapp/editor.dialogs.js:2451`), Insert/Delete Cells with
   shift, Row Height, Column Width, AutoFit (`:2792`), the sheet verbs
   (`:2295`), plus `#fx-insert` (`webapp/editor.html:502`), `#name-box-list`
   (`:498`), the three `.tb-align` buttons (`:316`, `:319`, `:322`) and
   `autoSum()` (`webapp/editor.selection.js:1320`, which has no DOM node at
   all). None is visible to `listCommands()`, `applyCommandRules()`, the
   read-only whitelist or the OS menu today. This is the same governance win
   `91` §Outcome-1 claims, and **it must be taken once, jointly** (§15).
8. **Nine toolbar buttons stop lying to the eye.** `#tb-fontcolor`,
   `#tb-fillcolor`, `#tb-rotate`, `#tb-merge`, `#tb-valign`, `#tb-border`,
   `#tb-freeze`, `#tb-sort` and `#tb-filter` all carry `aria-haspopup="true"`
   and draw no caret; only `#tb-numfmt` (`webapp/editor.html:401`) does, and
   `grep -n aria-haspopup webapp/editor.css` returns nothing. §8 makes the
   caret derived from the ARIA semantics so it cannot drift.
9. **Escape stops destroying a half-typed rule.** The panel close handler fires
   on any `Escape` while `activePanel` is set, with no check on `e.target`
   (`webapp/editor.core.js:10245-10248`), and `openPanel` clears the body on the
   next open (`:5160-5161`), so the draft is unrecoverable. Two panels guard
   their own fields (`webapp/editor.dialogs.js:1090`,
   `webapp/editor.core.js:4449`) and the guard was never generalised.

---

## 2. How this was measured, and what is *not* measured

**Our chrome is exact.** Every height, width, radius and duration attributed to
this product below is the declared value in `webapp/editor.css` or a sum of
declared values, cited to the line. Where a number is a sum it shows its
addends. **Nothing here was measured in a browser for this note** — this was a
design round, no build was run — so §6's width budget is **computed from §8's
metrics, not observed**. §15's gates are how it becomes a measurement.

**Google Sheets is estimated, everywhere, without exception.** Google publishes
no device-independent measurement for any part of the Sheets chrome and its DOM
class names are obfuscated and unversioned;
[88](88-EDITOR-CHROME-COMPOSITION.md) §0 records the same finding in those
words, and notes that its own Sheets header-band figures come from Luckysheet's
`rowHeaderWidth: 46, columnHeaderHeight: 20` and are "corroboration, not
measurement" (`docs/88:69-72`). What Google *does* publish, and what this note
leans on, is **content and behaviour**: the menu access keys, the tool-finder
binding, the sidebar-versus-dialog distinction, the Share dialog's exact
wording, version history's named-version limits, the comment assignment flow,
and the February 2023 Material 3 refresh. Every Sheets *pixel* in this note is
an inference and none of them is load-bearing, because **every number in §3, §6
and §8 is derived from our own tokens and our own asserted floors, not from a
guess about Google's**.

**Sourced claims about Google Sheets**, all accessed 2026-09-09:

| claim | source |
| --- | --- |
| Menu bar membership and access keys; `Ctrl+/` shortcut list; `Alt+/` tool finder; `Ctrl+Alt+M` comment; `Ctrl+Shift+F` compact controls | https://support.google.com/docs/answer/181110 |
| Document-level buttons sit between the menu-bar landmark and the menu bar; the three landmarks | https://support.google.com/docs/answer/6282736 |
| Tool finder: what it searches, and that it suggests related actions as you type | https://support.google.com/docs/answer/13466905 |
| "Sidebars do not suspend the server-side script while the dialog is open" — the sidebar/dialog line | https://developers.google.com/apps-script/guides/dialogs |
| Conditional formatting: "A toolbar will open to the right"; rules evaluated in list order | https://support.google.com/docs/answer/78413 |
| Chart editor opens on the right, Setup / Customize tabs | https://support.google.com/docs/answer/63824 |
| Data validation: the rules panel, "Add rule", Chip / Arrow / Plain text | https://support.google.com/docs/answer/186103 |
| Protected ranges: "A box will open on the right" | https://support.google.com/docs/answer/1218656 |
| Named ranges: "A menu opens on the right side of the spreadsheet" | https://support.google.com/docs/answer/63175 |
| Version history from the "Last edit" button; named versions; **15 per spreadsheet**; "Only show named versions"; "Show unmodified rows"; per-person colour | https://support.google.com/docs/answer/190843 |
| Comments: assign via @, the "Assign to \<name\>" checkbox, Resolve, "Show all comments" | https://support.google.com/docs/answer/65129 |
| Share dialog: Viewer / Commenter / Editor, "General access", Restricted vs Anyone with the link, the two gear checkboxes | https://support.google.com/docs/answer/2494822 |
| Filter views are private; "At the top right, click Save View" | https://support.google.com/docs/answer/3540681 |
| Explore is at the bottom right | https://support.google.com/a/users/answer/9308959 |
| The Feb 2023 refresh: simplified UI at the top of the file, updated toolbar groupings, companion bar hidden by default | https://workspace.google.com/blog/productivity-collaboration/more-flow-less-work-smart-canvas (published 2023-02-24) |
| Material 3 easing and duration token values | https://raw.githubusercontent.com/androidx/androidx/androidx-main/compose/material3/material3/src/commonMain/kotlin/androidx/compose/material3/tokens/MotionTokens.kt (the generated canonical form of https://m3.material.io/styles/motion/easing-and-duration/tokens-specs, which is client-rendered and does not fetch as text) |
| Snackbar geometry: 48 dp tall, 288–568 dp wide, 24 dp inset, one action, never "Dismiss" | https://m1.material.io/components/snackbars-toasts.html |
| Sheets' narrow-window overflow is a cliff users read as feature removal | https://support.google.com/docs/thread/174270123 and https://support.google.com/docs/thread/410076953 |
| **Negative evidence:** suggest-edits is a Google **Docs** page; Sheets has no suggestion mode | https://support.google.com/docs/answer/6033474, corroborated by https://support.google.com/docs/thread/14593245 and https://support.google.com/docs/thread/28604653 |

**Four things are estimated and are marked again where they appear:** every
Sheets pixel; Sheets' toolbar left-to-right order (no Google page enumerates
it — the *set* is confirmed by https://support.google.com/a/users/answer/9300022,
the sequence is reconstructed); Sheets' overflow breakpoints (the *mechanism* is
observed, the widths are not published); and every motion value except the M3
tokens, which are cited.

**Three corrections to the material this round was briefed with**, each checked
at source, because a design built on them would have been wrong:

- **`canShare` is not off in every preset.** `MODE_PRESETS` sets it `true` in
  `standalone` (`webapp/editor.core.js:955`) and `desktop` (`:958`), `false` in
  `embedded`, `wopi` and `viewer`. The stale comment claiming otherwise is at
  `:1352`. The Share pill is live on the two mounts that matter.
- **The chart panel already has a chart-type control.** `panelLabel(body,
  "Type")` at `webapp/editor.core.js:4429` with a `CHART_KINDS` select wired to
  `applyChart` at `:4437`. Insert-then-configure is therefore half built.
- **The bottom bar is already the Sheets bottom bar.** `UX-CHR-07` pinned the
  `＋` and `☰` controls outside the tab scroller — the rail adopts the very
  nodes `renderTabs()` builds and is inserted *before* `#sheet-tabs`
  (`webapp/editor.sheets.js:283`) — for exactly the reason Sheets pins them:
  measured, they went out of reach at twelve sheets on a 1280 px laptop
  (`webapp/editor.sheets.js:126-136`).

**Two derived claims, not read.** That the menu bar can simply *be* the Sheets
menu bar is derived from the fact that seven of our eight menu names match
Sheets' nine and from `menuModel()` filtering on `.oc-cmd-hidden` rather than
visibility (`webapp/editor.selection.js:1128-1131`); it was not driven in a
browser. And **nobody has ever run `listCommands().length`** —
`webapp/docs.html:353` advertises 189, `docs/91` §3.12 asserts 188, and the six
Download rows are generated at runtime from `writable_extensions()`
(`crates/casual-calc-wasm/src/io.rs:267`), so the number is build-dependent. It
does not enter a gate in this note until it is measured.

---

## 3. The region stack

Declared heights, mouse metrics, web chrome, header shown. Every figure is
`content + 1 px hairline` where the region has one, and every band is
`flex: 0 0 auto` in one column (`webapp/editor.css:227-233`), so nothing else
contributes.

### 3.1 Today

| region | height | source |
| --- | --- | --- |
| `.app-header` | 52 + 1 = **53** | `webapp/editor.css:303`, `:305` |
| `.menubar` | 30 + 1 = **31** | `webapp/editor.css:415`, `:416` |
| `.toolbar` | 41 + 1 = **42** | `webapp/editor.css:356`, `:358` |
| `.formula-bar` | 35 + 1 = **36** | `webapp/editor.css:994`, `:995` |
| | **162 above the grid** | |
| `.bottom-bar` | 34 + 1 = **35** | `webapp/editor.css:1037`, `:1038` |
| | **197 total** | |

### 3.2 After

```
  app bar     36  =  2 + 32 + 2            no hairline
                        32 = .tb-icon, editor.css:779-780
+ menu bar    28  =  2 + 24 + 2            no hairline
                        24 = .menu-top label box, editor.css:530-534
+ toolbar     40  =  4 + 32 + 4            no hairline
                        32 = 2 + 28 + 2, the tonal container
                        28 = .tb-btn, editor.css:763
+ formula bar 36  = 35 + 1                 unchanged, editor.css:994-995
= 140 px above the grid
+ bottom bar  35  = 34 + 1                 unchanged, editor.css:1037-1038
= 175 px of chrome
```

Three of the five hairlines go. They are absorbed by a **tonal step** — the app
bar, menu bar and toolbar band sit on `--oc-chrome-ground` and the grid is
white — which is Sheets' first visual rule, elevation and tone instead of
borders (https://workspace.google.com/blog/productivity-collaboration/more-flow-less-work-smart-canvas,
2023-02-24). The two that survive earn it: on both sides of the formula bar's
seam and the bottom bar's seam the surface is the same white, so there is no
tonal step to do the work.

### 3.3 Content share

| | today | ribbon Simplified | ribbon Classic | **Sheets** | Sheets, compact |
| --- | --- | --- | --- | --- | --- |
| above the grid | 162 | 144 | 196 | **140** | 76 |
| total chrome | 197 | 179 | 231 | **175** | 111 |
| grid at 1440 × 900 | 703 | 721 | 669 | **725** | 789 |
| content share | 78.1 % | 80.1 % | 74.3 % | **80.6 %** | 87.7 % |
| grid at 1920 × 1080 | 883 (81.8 %) | 901 (83.4 %) | 849 (78.6 %) | **905 (83.8 %)** | 969 (89.7 %) |

Today's 162/197 is `docs/91` §1.1's arithmetic, re-verified line by line against
the current tree; it holds exactly. 144/196 is `docs/91` §1.2. Compact controls
folds the app bar and the menu bar — 64 px — and is the one gesture the ribbon
has no direct equivalent for.

**Where the 22 px against today comes from:** −17 on the app bar (52 → 36,
because the strip carries no product mark and no file action, and
`webapp/editor.css:309-311` and `webapp/editor.html:80-92` record both
deletions), −3 on the menu bar, −2 on the toolbar, 0 on the formula bar, 0 on
the bottom bar.

**Honest cost line.** The tonal container costs 6 px over a bare 34 px band. It
buys the one thing that makes the header read as a single designed surface
rather than three stacked strips, and it is what lets every group capsule and
every filled pill go. `--oc-chrome-toolbar-inset: 0` gives a flat 34 px band,
134 above the grid, 81.2 % at 1440 × 900 — a token change with no other
consequence.

### 3.4 What each region carries

| region | carries | changed from today |
| --- | --- | --- |
| app bar | mark, document name (inline-editable), **one** save state, import chip, tool finder — gap — last-edit link, comment history, presence avatars, Share pill, `⋮`, compact caret | §4 |
| menu bar | tool-finder magnifier, File · Edit · View · Insert · Format · Data · Tools · Help, compact caret pinned right | items unchanged (§5); `#presence` moves out |
| toolbar | one row, eight groups, hairline dividers, one rounded tonal container, `⌄` pinned right | §6 |
| formula bar | Name Box, seam, decorative `fx`, input, expander | **untouched** |
| grid | canvas, a11y mirror, live region, editor, scrollbars, **snackbar** anchored inside `.grid-wrap` | snackbar is new |
| bottom bar | `＋` `☰` rail, tab strip, `#cell-mode`, `#tb-status`, `#sel-stats`, `#zoom-widget`, locale picker, `#autosave-state` | `.bottom-status` retires; `#sel-stats` becomes a menu button |

`.bottom-status` (`webapp/editor.html:609`) is the only deletion, and it becomes
hover detail on the app bar's save state. It carries no command id, so
`listCommands()` does not change.

The formula bar is untouched **deliberately**: `UX-CHR-08` already shipped
Sheets' composition — one flat bar, no independent control borders, a single
seam after the Name Box, a decorative `fx`, the expander at the right end
(`webapp/editor.css:990-1032`; `docs/88:388-405`). The one thing missing is a
command id on `#fx-insert` and `#name-box-list`, and that is a shared row, not a
chrome change.

---

## 4. The app bar, and the collaboration surfaces

One 36 px row, replacing `.app-header` (`webapp/editor.html:46-104`) and
absorbing what the menu bar's right end carries today. This project runs a
clustered OT server with presence, resume and relay (`ADR-011`/`012`/`014`/`017`;
[63](63-COLLABORATION-RELAY.md), [61](61-COLLABORATION-RESUME.md)). **This is
the chrome that makes it visible.**

### 4.1 The row, left to right

| # | slot | node | id | note |
| --- | --- | --- | --- | --- |
| 1 | mark | `.brand-logo` (`webapp/editor.html:56`) | — | 24 px glyph in a 40 px target, filled `--oc-accent-color`. Never a vendor green. `tests/browser/editor.branding.spec.mjs:57` boots `?brand=Ledgerly` and reads `.app-header` |
| 2 | document name | `#doc-name` → `#doc-rename` (`:72`, `:73`) | `file.rename` *(to mint)* | Click swaps the button for the input; Enter or blur commits. Already Sheets' behaviour; the verb has no command id today |
| 3 | **save state** | `#doc-state` (`:77`) | — | `role="status" aria-live="polite"`. "Saving… → All changes saved → ✓", or "Not saved — \<reason\>". §4.3 |
| 4 | import chip | new | `file.import-report` *(to mint)* | Present only when `session_import_summary()` is non-empty. §7 |
| 5 | tool finder | new | `help.search-the-menus` *(to mint)* | Magnifier expanding to a 320 px field. `Alt+/`, `Ctrl+K` |
| | — flexible gap — | | | |
| 6 | last edit | new | `file.version-history` *(proxy)* | "Last edit was …", carrying a dot when the document changed since this session last looked |
| 7 | comment history | new | `insert.note` *(proxy)* | Opens the Comments panel in list mode |
| 8 | **presence** | `#presence` (`:181`) **relocated** | — | 18 px faces, `margin-left: -5px`, max 3 then "+N" (`webapp/editor.css:463-473`). §4.2 |
| 9 | **Share** | new pill | `file.share` *(proxy)* | Tonal `--oc-accent-soft` pill. Hidden — not disabled — when `canShare` is false |
| 10 | `⋮` | new | — | Holds `#tb-settings` (`:95`, keeping its `toolbar.settings` id), `file.properties`, `help.keyboard-shortcuts` |
| 11 | compact caret | `#hdr-collapse` (`:194`) | `view.compact-controls` *(to mint)* | Rebound to fold **both** this row and the menu bar. `Ctrl+F1` |

### 4.2 Presence: it populates, it does not appear

`renderPresence()` sets `box.hidden = true` whenever `collabSession` is falsy
(`webapp/editor.presence.js:260-263`), and `#presence` is authored inside
`#menubar` (`webapp/editor.html:181`). So the entry point to the collaborative
product is invisible until collaboration is already happening — a circular
dependency that guarantees discovery never occurs — and it lives in a bar that
`?hide=menubar` can take away.

**After:** the avatar stack is a permanent fixture of the app bar. Alone, it
shows one face — yours. Clicking a face scrolls the grid to that person's
selection through the `ensureVisible` already imported at
`webapp/editor.presence.js:24`. The existing roster popover
(`#presence-menu`, `webapp/editor.html:188`) becomes the stack's detail view,
unchanged, with its `role="menu" aria-label="Collaborators"` intact. Join and
leave are announced through `#grid-live` (`webapp/editor.html:560`), **never**
by putting `aria-live` on the roster — a live region on a presence row
announces every cursor move, at 60 Hz during a drag-select.

**The move is a two-step, and it has a shipped assertion in front of it.**
`.oc-hide-menubar .menubar > *:not(.presence)` (`webapp/editor.css:267`) exists
precisely so a host that hides the menus keeps the roster, and
`tests/browser/editor.chrome-regions.spec.mjs:43` asserts it by name. So: move
`#presence` first, through the reversible `chromeHome` / `placeNativeChrome`
mechanism (`webapp/editor.core.js:1310`), then re-author the region rules so
that **in this chrome** `hide=menubar` leaves the app bar and the roster
standing, and `hide=header` does not silently take the roster with it. Both
directions get a gate (§16).

### 4.3 One save state

There is no Save button in Sheets and there is none here on the web mount
(`file.save` is desktop-only — `CHROME_ONLY.native` matches `/^file\.save$/` at
`webapp/editor.core.js:1377-1379`). In its place, one ambient span:

| state | text | driven by |
| --- | --- | --- |
| writing | `Saving…` | the draft scheduler in `webapp/editor.drafts.js` and the collab send loop |
| settled | `All changes saved`, collapsing to ✓ after 2 s | both settled |
| failed | `Not saved — <reason>`, `--oc-danger-color`, does not collapse | either faults |

**Event-driven, not polled.** Today `#doc-state` is written by a `publish()`
closure on `setInterval(publish, 250)` (`webapp/editor.core.js:9188`, `:9193`)
reporting `isDirty()`, which means "changes that have not been written out" —
a browser-tab fact, not a safety fact. `.bottom-status`'s "Local only · nothing
uploaded" (`webapp/editor.html:609`) becomes hover detail on this span.
`#autosave-state` (`:616`) keeps only its failure role and its `role="status"`.
`Ctrl+S` and `File ▸ Download` keep reporting separately — **exporting a file is
a different promise from persisting the document**, and collapsing them is how a
user comes to believe a download happened because a cloud icon went grey.

The transition is §9's item 4 and it is the smallest surface in the chrome and
the most load-bearing: the entire promise of "you never lose work" is delivered
by one 140 ms text crossfade that does not draw the eye.

### 4.4 What is *not* in this row

**Meet / video.** Sheets puts a camera pill here. We have a collaboration
server, presence and a relay, and no call. Not drawn.

**Star and Move-to-folder.** Sheets has Drive behind them
(https://support.google.com/docs/answer/6282736). We do not. The two slots are
correctly empty and the `⋮` carries document-level actions instead.

**A wordmark, a product name, or a vendor colour.** `?brand=Ledgerly` is
asserted against `.app-header`, `#menubar` and `.bottom-bar`
(`tests/browser/editor.branding.spec.mjs:57`). No Google green, ever.

**`#tb-status` does not move here.** It stays in the bottom bar, unrenamed and
undelayed. `tests/browser/editor.chrome-composition.spec.mjs:112` pins it across
the page, `?chrome=native` and the desktop shell, and 84 of the 88 browser specs
boot on it reading `/^engine v\d/`.

### 4.5 The rest of the collaboration surface, and one honest gap

| Sheets surface | ours | where it lands |
| --- | --- | --- |
| Avatar row, colour per person | `collabRoster`, `participantName`, per-participant colour already painted onto remote cursors (`webapp/editor.paint.js:904`, tag at `:920-925`) | §4.2 |
| Share dialog: 3 roles × 2 link states × 2 checkboxes | `shareDialog()`, `file.share` | opened as a panel, §7 |
| Comments with @mention and "Assign to \<name\>" | per-cell panel only; **asks the user to type their name** into a `localStorage` field (`webapp/editor.core.js:5545-5549`; `commentAuthor()`/`setCommentAuthor()` at `webapp/editor.sheets.js:498-504`) while the roster's name for that same person is already on their cursor | §7, Comments panel |
| Version history with named versions and per-person colour | `session_versions()` (`webapp/editor.core.js:5213`), `cellAuthor()` over `session_cell_author` (`:5203-5206`), `buildHistoryPanel` (`:5208`) | §7 |
| Cell "Show edit history" on the context menu | `session_cell_author` exists; [89](89-CHANGE-ATTRIBUTION-AND-TRACKING.md) holds the model | §16, a row of its own |
| **Suggestion mode** | — | **Sheets has none.** §13 |

Two bounds this chrome must not overstate. **Version history here is a snapshot,
not a replayed log** — [83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) records
that the collaboration op log has no timestamps and no per-revision author and
is evicted after the last participant leaves. And **undo is per-user and travels
to peers** ([69](69-COLLABORATIVE-UNDO-POLICY.md)), which is why §9's snackbar
dismisses on any subsequent local edit rather than sitting there for six
seconds offering to reverse the wrong operation.

---

## 5. The menus

### 5.1 The rule the whole section obeys

**A command id never moves.** `commandId()` slugifies the English label path
(`webapp/editor.selection.js:1015-1023`), and both `CAPABILITY_COMMANDS`
(`webapp/editor.core.js:1336-1358`) and `READ_ONLY_SAFE` (`:4993-5008`) match
ids **by regex**. So re-slugging `file.download.csv-csv` into a Sheets menu path
un-gates `canSaveAs`, and re-slugging anything out of `/^view\.zoom/` or
`/^tools\.calculation/` takes zoom or recalculation away from every viewer. A UI
change becoming a security defect, with nothing red anywhere. `ADR-026` clause
(4) exists for exactly this.

Therefore, three notations in the tables below:

- **owner** — the row carries `data-oc-command="<id>"`. Exactly one node per id.
- **proxy** — the row carries `data-oc-proxy="<id>"` and clicks the owner. This
  is how Sheets' menu geography is honoured without moving an id. The pattern
  `MENUS` already uses fourteen times.
- **to mint** — no id exists today. Every one of these is minted **once,
  jointly with the ribbon workflow**, into the frozen manifest (§15). A verb
  with two ids is a verb one capability regex matches and the other does not.

And a second rule that shapes the shape: **Sheets' nesting is copied only where
it does not move an existing id.** Sheets writes `View ▸ Show ▸ Formula bar`
and `Format ▸ Text ▸ Bold`; nesting our existing `view.gridlines` and
`format.bold` under those would re-slug them to `view.show.gridlines` and
`format.text.bold`. Those groups are drawn **flat with a separator** instead.
The visual grouping survives; the id does not move.

Finally: **no new element may carry an `id` starting `tb-`.** The boot sweep
stamps every such node with `toolbar.<rest>` (`webapp/editor.core.js:11019-11021`),
which is how `toolbar.status`, `toolbar.numfmt-label`, `toolbar.more-flyout` and
the two combo carets are in `listCommands()` today.

### 5.2 File

Sheets' File menu is ~17 items and holds everything Excel's nine-page backstage
keeps. Ours is the same shape.

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| New | `file.new` | | owner | `canOpen` |
| Open… | `file.open` | Ctrl+O | owner | clicks `#tb-open` (`webapp/editor.html:213`) |
| Rename | `file.rename` | | **to mint** | proxies the app-bar title field; Sheets has this and we have no id for it |
| Share… | `file.share` | | owner | `canShare` — true in `standalone` and `desktop` (`webapp/editor.core.js:955`, `:958`); the app-bar pill proxies this |
| — | | | sep | |
| Save | `file.save` | Ctrl+S | owner | desktop only, `CHROME_ONLY.native` (`:1377-1379`). Correctly absent in a browser tab, as in Sheets |
| Download ▸ | `file.download` | | owner | **do not regroup or reword** — `canSaveAs` matches `/^file\.download/` and the six leaves are slugged from these exact words |
| ↳ Same format as opened | `file.download.same-format-as-opened` | | owner | generated (`webapp/editor.sheets.js:619`) |
| ↳ Excel (.xlsx) · Excel macro-enabled (.xlsm) · OpenDocument (.ods) · CSV (.csv) · Tab-separated (.tsv) · Pipe-separated (.psv) | `file.download.*` | | owner | generated from `writable_extensions()` (`webapp/editor.sheets.js:603-608`; `crates/casual-calc-wasm/src/io.rs:267`) |
| Export as PDF… | `file.export-as-pdf` | | owner | ours. Folding it into Download would re-slug it into the `canSaveAs` regex — a capability change disguised as a tidy-up |
| — | | | sep | |
| Print… | `file.print` | Ctrl+P | owner | read-only safe |
| Page setup… | `file.page-setup` | | owner | opens the Page setup panel. Ours — Sheets has no page-oriented surface |
| Page break here | `file.page-break-here` | | owner | |
| — | | | sep | |
| Version history | `file.version-history` | Ctrl+Alt+Shift+H | owner | **flat**, not Sheets' `Version history ▸ See version history`; nesting would re-slug it |
| Import report… | `file.import-report` | | **to mint** | §7. Today the loss summary is appended to `#tb-status` (`webapp/editor.dialogs.js:2075-2087`) |
| Properties… | `file.properties` | | owner | Sheets calls this *Details*; keep the English label, retitle through `command.<id>` (`webapp/editor.i18n.js:118-124`) |
| Settings… | `tools.settings` | | **proxy** | Sheets puts spreadsheet settings in File. Our owner is in Tools; this is a fourth *route*, not a fourth id |

### 5.3 Edit

Sheets' Edit menu is nine items. Excel's Home tab alone carries more than that
in Clipboard and Editing.

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| Undo | `edit.undo` | Ctrl+Z | owner | |
| Redo | `edit.redo` | Ctrl+Shift+Z | owner | the toolbar tooltip says Ctrl+Y (`webapp/editor.html:248`); both chords work, and §11's rebuilt shortcut sheet settles the label |
| — | | | sep | |
| Cut | `edit.cut` | Ctrl+X | owner | |
| Copy | `edit.copy` | Ctrl+C | owner | read-only safe |
| Paste | `edit.paste` | Ctrl+V | owner | |
| Paste special ▸ | `edit.paste-special` | Ctrl+Alt+V | **to mint** | Values only · Format only · Formula only · Transposed — four more ids. These exist today only inside `cellMenu()`'s submenu (`webapp/editor.dialogs.js:2479-2485`) with no ids. Minting them retires `pasteSpecialDialog()`, whose empty-clipboard early return at `:2091` is `docs/82:28`'s "Paste special / did not open" |
| — | | | sep | |
| Move ▸ | `data.group` — **no** | | *not drawn* | Sheets' Move ▸ Row up/down has no engine verb here. §13 |
| Delete ▸ Rows · Columns | `insert.delete-rows` · `insert.delete-columns` | | **proxy** | Sheets puts these under Edit; the owners stay where their ids say |
| Delete ▸ Cells and shift up · Cells and shift left | `edit.delete-cells` | | **to mint** | `cellMenu()`-only today (`webapp/editor.dialogs.js:2523-2527`) |
| Clear ▸ Values · Formatting · All | `edit.clear.*` | | owner | `webapp/editor.core.js:9722-9724` |
| Fill ▸ Fill down · Fill right · Fill series · Growth series · Copy cells · Formatting only · Without formatting | `edit.fill.*` | Ctrl+D, Ctrl+R | owner | ours. Sheets has only the drag handle, and a mode reachable only by dragging is one most people never find. Placed where Sheets puts Move ▸ |
| — | | | sep | |
| Find & replace… | `edit.find-replace` | Ctrl+F | owner | opens the find **bar** (`webapp/editor.find.js`), not a modal. Sheets uses a dialog; ours is better and stays |
| Select all | `edit.select-all` | Ctrl+A | owner | read-only safe |

### 5.4 View

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| Formula bar | `view.formula-bar` | | **to mint** | mechanism exists — `.oc-hide-formulabar` (`webapp/editor.css:253`) — with no user-facing command |
| Gridlines | `view.gridlines` | | owner | |
| Cell markings | `view.cell-markings` | | owner | Excel calls them Headings; the word *headers* is reserved for the page header (`webapp/editor.core.js:9757`) |
| Formulas instead of results | `view.formulas-instead-of-results` | Ctrl+\` | owner | |
| Zero values | `view.zero-values` | | owner | ours |
| — | | | sep | *(this separator is Sheets'* `Show ▸` *submenu, drawn flat)* |
| Freeze ▸ Up to selection · Top row · First column · Unfreeze | `view.freeze.*` | | owner | Sheets puts Freeze in View and so do we, already |
| Group ▸ | `data.group` | | **proxy** | Sheets: View ▸ Group. Owner stays in Data |
| Comments ▸ Show all comments | `insert.note` | Ctrl+Alt+Shift+A | **proxy** | opens the panel in list mode |
| Hidden sheets ▸ | `view.hidden-sheets` | | **to mint** | the list already exists behind the bottom bar's `☰` (`webapp/editor.core.js:7061-7085`); this needs an id and a proxy, not new behaviour |
| — | | | sep | |
| Zoom ▸ 50 · 75 · 100 · 150 · 200 % | `view.zoom.*` | Ctrl+Alt+0 | owner | **do not move or rename** — `READ_ONLY_SAFE` matches `/^view\.zoom/` |
| Theme ▸ Auto · Light · Dark | `view.theme.*` | | owner | ours; Sheets has no desktop dark theme, so §8's dark values are an invention and [49](49-DESIGN-SYSTEM.md) owns them |
| **Layout ▸ Auto · Ribbon · Toolbar** | `view.layout.*` | | **to mint** | §12. The chrome chooser |
| Compact controls | `view.compact-controls` | Ctrl+F1 | **to mint** | folds the app bar **and** the menu bar, keeps the toolbar, persists. §12.7 explains why not Sheets' Ctrl+Shift+F |
| — | | | sep | |
| Settings… | `view.settings` | | owner | duplicate verb, kept |

### 5.5 Insert

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| Cells ▸ shift right · shift down | `insert.cells` | | **to mint** | `cellMenu()`-only (`webapp/editor.dialogs.js:2523-2526`). Sheets leads its Insert menu with this |
| Rows above · Rows below | `insert.rows-above` · `insert.rows-below` | | owner | |
| Columns left · Columns right | `insert.columns-left` · `insert.columns-right` | | owner | |
| Delete rows · Delete columns | `insert.delete-rows` · `insert.delete-columns` | | owner | Edit ▸ Delete carries the proxies a Sheets user looks for |
| — | | | sep | |
| Sheet | `insert.sheet` | Shift+F11 | **to mint** | the `＋` rail button (`webapp/editor.core.js:7045-7058`) and Shift+F11 (`:8650`-region) both do this and neither carries an id, so "add a sheet" is unreachable from the SDK and the OS menu |
| Chart ▸ Column · Bar · Line · Area · Pie · Doughnut · Scatter | `insert.chart.*` | | owner | **kept**, with all seven leaf ids; retiring them shrinks published SDK surface. The argument-free route is the toolbar button |
| PivotTable… | `insert.pivottable` | | owner | |
| Table… | `insert.table` | Ctrl+T | owner | |
| Function ▸ SUM · AVERAGE · COUNT · MAX · MIN · All functions… | `insert.function` | | **to mint** | the engine half exists with **no DOM node at all**: `autoSum()` (`webapp/editor.selection.js:1320`) is keyboard-only, and `#fx-insert` (`webapp/editor.html:502`) carries no `data-oc-command`. Two ids; the cheapest governance win in the inventory |
| — | | | sep | |
| Link | `insert.hyperlink` | Ctrl+K | owner | opens an anchored bubble, §7 |
| Comment | `insert.note` | Ctrl+Alt+M | owner | labelled "Note" today (`webapp/editor.core.js:9809`) while the panel it opens is titled "Comments" (`:5159`) and the context menu says "Insert comment" (`webapp/editor.dialogs.js:2565`) — three names, one surface. Keep the id, fix the wording in the catalogue |
| Dropdown | `data.data-validation` | | **proxy** | Sheets: Insert ▸ Dropdown creates a validation rule |

### 5.6 Format

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| Number ▸ Automatic · Number (0.00) · Thousands (#,##0) · Percent (0%) · Currency · Short date · Time · Scientific · Text | `format.number.*` | | owner | **nine** against the toolbar's fifteen (`#numfmt-menu`, `webapp/editor.html:405`). §13 refuses to unify them |
| Custom number format… | `format.custom-number-format` | | owner | stays a modal — Sheets keeps this one as a dialog too |
| — | | | sep | |
| Bold · Italic · Underline · Strikethrough | `format.bold` `.italic` `.underline` `.strikethrough` | Ctrl+B/I/U | owner | flat, not under Sheets' `Text ▸`; nesting re-slugs all four |
| Superscript · Subscript | `format.superscript` `.subscript` | | owner | ours; mutually exclusive — one `vertAlign` on the font |
| — | | | sep | |
| Alignment ▸ Left · Center · Right · Fill (repeat text) · Justify · Center across selection · Distributed · Clear (General) — Top · Middle · Bottom · Justify (vertical) · Distributed (vertical) | `format.alignment.*` | | owner | thirteen against Sheets' six. The extras are OOXML modes the importer must round-trip |
| Text overflow ▸ Overflow · Wrap · Clip | `format.text-overflow.*` | | owner | Sheets' *Wrapping* exactly — three named states, where Excel has a toggle plus a dialog checkbox |
| Rotation ▸ | `toolbar.rotate` | | **proxy** | rotation has no menu id today, so a Sheets user looking in Format finds nothing |
| Merge cells | `format.merge-cells` | | owner | proxies `#tb-merge` |
| — | | | sep | |
| Conditional formatting… | `format.conditional-formatting` | | owner | opens the panel in **edit** state |
| Conditional formatting rules… | `format.conditional-formatting-rules` | | owner | **repointed, not deleted** — opens the *same* panel in **list** state. §7 |
| Cell styles… | `format.cell-styles` | | owner | placed where Sheets puts Alternating colours |
| Convert to table… | `insert.table` | | **proxy** | Sheets: Format ▸ Convert to table |
| — | | | sep | |
| Trace ▸ precedents · dependents · clear arrows | `format.trace.*` | | owner | ours (Excel's Formulas ▸ Trace). Kept in Format because that is where the id says it lives |
| Protection ▸ Locked · Hide formula · Protect this sheet | `format.protection.*` | | owner | Data ▸ Protected sheets and ranges carries the proxy |
| Cell format… | `format.format-cells` | Ctrl+1 | **to mint** | `formatCellsDialog()` (`webapp/editor.dialogs.js:141`) is reachable only from `cellMenu()` and Ctrl+1 and carries no id. Sheets has no equivalent surface at all; ours becomes a **panel**, §7 |
| Clear formatting | `format.clear-formatting` | Ctrl+\\ | owner | |

### 5.7 Data

| item | id | kind | note |
| --- | --- | --- | --- |
| Sort range ▸ A → Z · Z → A · Custom sort… | `data.sort-range.*` | owner | custom sort caps keys at three by a literal (`webapp/editor.dialogs.js:641`) while `session_sort_range_multi` takes a vector (`crates/casual-calc-wasm/src/structural.rs:415-423`) — `docs/92` C-8 |
| Filter | `data.filter` | owner | Sheets calls this *Create a filter*; retitle through i18n |
| Clear all filters | `data.clear-all-filters` | owner | |
| Clear my view | `data.clear-my-view` | owner | ours, and the closest thing we have to Sheets' filter views. It is the command the filter-view banner's ✕ proxies (§7) |
| — | | sep | |
| Data validation… | `data.data-validation` | owner | |
| Remove duplicates… | `data.remove-duplicates` | owner | Sheets groups this under `Data cleanup ▸`; flat plus a separator here |
| Convert text to numbers | `data.convert-text-to-numbers` | owner | ours (`DATA-NT-01`). Grouped with Remove duplicates because that is where the problem comes from |
| Text to columns… | `data.text-to-columns` | owner | Sheets: *Split text to columns* |
| — | | sep | |
| Named ranges | `tools.name-manager` | **proxy** | Sheets puts named ranges in Data; moving the owner would mint `data.named-ranges` and orphan any host dispatching the old id |
| Protected sheets and ranges | `format.protection.protect-this-sheet` | **proxy** | opens the Protected ranges panel |
| Column stats… | `data.column-stats` | owner | ours **and** Sheets' — both products independently chose it, in the same menu |
| — | | sep | |
| PivotTable fields… | `data.pivottable-fields` | owner | same dialog as `insert.pivottable`; duplicate verb, two ids, left alone |
| Refresh pivot · Refresh all pivots | `data.refresh-pivot` · `data.refresh-all-pivots` | owner | Alt+F5, Ctrl+Alt+F5 |
| — | | sep | |
| Hide rows · Hide columns | `data.hide-rows` · `data.hide-columns` | owner | |
| Group ▸ rows · columns · ungroup · Expand all · Collapse all · Show level 1 · Show level 2 | `data.group.*` | owner | View ▸ Group is the proxy |
| Unhide rows/columns in selection | `data.unhide-rows-columns-in-selection` | owner | |
| Unhide all rows and columns | `data.unhide-all-rows-and-columns` | owner | |
| Subtotal… | `data.subtotal` | **to mint** | `docs/92` C-12, cost *small*: `session_group` and `session_show_outline_level` already drive the outline (`crates/casual-calc-wasm/src/axis.rs:1048-1179`). Listed because Data is where it belongs, not because this chrome must ship it |

### 5.8 Tools

| item | id | kind | note |
| --- | --- | --- | --- |
| Settings… | `tools.settings` | owner | **owner**; File ▸ Settings is the proxy a Sheets user reaches for |
| Name manager… | `tools.name-manager` | owner | **owner**; Data ▸ Named ranges is the proxy |
| Calculation ▸ Automatic · Manual · Calculate now (F9) | `tools.calculation.*` | owner | **do not move into File ▸ Settings**, however Sheets-shaped that would be: `READ_ONLY_SAFE` matches `/^tools\.calculation/`, so re-slugging these takes recalculation away from every viewer. The Settings panel gains a Calculation section that **proxies** these three, and gains `docs/92` C-11's iterative-calculation field once one wasm export exists (`WorkbookSettings::iteration()`, `crates/casual-calc-model/src/workbook.rs:252`) |

### 5.9 Help

| item | id | shortcut | kind | note |
| --- | --- | --- | --- | --- |
| Search the menus | `help.search-the-menus` | Alt+/ | **to mint** | the tool finder. Sheets puts it here as well as in the chrome |
| Function list | `help.function-list` | | **to mint** | proxies `#fx-insert`'s catalogue, which has no id today |
| Keyboard shortcuts | `help.keyboard-shortcuts` | Ctrl+/ | owner | rebuilt from `menuModel()` rather than the 17-row literal at `webapp/editor.core.js:9569-9588`, which is a second source of truth maintained against ~129 menu leaves |
| About ${BRAND} | `help.about-<brand-slug>` | | owner | **the one brand-dependent id in the build** — minted from `` `About ${BRAND}` `` (`webapp/editor.core.js:9948`), so the slug changes with `?brand=`. Any manifest gate must special-case it |

### 5.10 Extensions — deliberately not drawn

Sheets has a ninth menu: Add-ons, Apps Script, AppSheet, Macros. We have no
automation of any kind — [92](92-COMPETITIVE-GAP-ANALYSIS.md) calls that the
largest categorical loss — and a menu of four permanently disabled rows teaches
a user that this product's menus lie, which costs more than the missing menu
does.

**Decision: no Extensions menu ships in this chrome. Its position between Tools
and Help is reserved so its arrival reorders nothing, and the trigger is
automation existing, not the chrome shipping.** Safe to omit on one further
count: our menu-bar mnemonics are derived per locale at relabel time from the
translated label (`webapp/editor.i18n.js:87-108`), not published, so Sheets'
`Alt+N` has no counterpart to break here.

### 5.11 The count

Eight menus, ~129 leaves that exist today, **21 ids to mint** across the tables
above (`file.rename`, `file.import-report`, `edit.paste-special` + 4 variants,
`edit.delete-cells` + 1, `view.formula-bar`, `view.hidden-sheets`,
`view.layout.*` × 3, `view.compact-controls`, `insert.cells` + 1,
`insert.sheet`, `insert.function` + 1, `format.format-cells`, `data.subtotal`,
`help.search-the-menus`, `help.function-list`), and **10 proxies** that mint
nothing. The ~40 remaining ungoverned context-menu verbs (`headerMenu`
`webapp/editor.dialogs.js:2792`, `sheetMenu` `:2295`) are the ribbon's half of
the same job and are not minted here — §15's wave rule says why.

---

## 6. The toolbar

**Thirty command slots plus one overflow chevron. One row, never two, never
tabbed.**

### 6.1 The selection rule

A command earns a permanent slot only by passing **all four** tests. The tests
are the specification; the thirty slots are what falls out of running them over
the inventory.

**(a) Frequency.** A median user invokes it more than once per editing session.
Not "belongs to a family already on the bar" — there is no taxonomy here, so
Comma style does not get a seat because Currency and Percent have one.

**(b) Reversibility.** It routes through `EditOperation` and therefore through
history (`crates/casual-calc-wasm/src/axis.rs:1041` is that path;
`crates/casual-calc-wasm/src/lib.rs:2189` is the shape of the proof). Anything
whose loss happens outside the model — a file written, a session replaced, a
lossy export — is a menu item with a confirm, however often it is used.

**(c) Immediacy.** It is meaningful on the current selection with no further
argument. If the first thing it must ask is *which range / which rule / which
type / which target / which file*, it is a menu item that opens a panel.

**(d) No duplicate readout.** No other permanently-visible control already both
shows and sets that state.

**Where a failure sends it, and these are different destinations:**

| fails | destination | why not the other one |
| --- | --- | --- |
| (c) | a **menu**, in the place Sheets puts it, opening a **panel** | a user who opens `⌄ More` expects controls that act on what is selected; a control that would open a configuration surface makes the overflow into a second, worse menu |
| (b) | a **menu**, always, regardless of frequency | |
| (a) or (d) only | `⌄ More`, **permanently resident**, in original group order with its separators | the node must stay in the DOM — `listCommands()` reads the live DOM (`webapp/editor.selection.js:1040-1046`) |
| nothing — the window is narrow | `⌄ More`, **temporarily**, by §6.4's authored ladder | |

**What the rule disqualifies from today's bar**, each with the clause: `#tb-freeze`
(`webapp/editor.html:437`) and `#tb-sort` (`:450`) fail (c) — both need a target
or a key chosen; `#tb-comma` (`:387`) fails (a) and (d) — `123 ▾` already sets
and shows the format; `#tb-indent-less` / `#tb-indent-more` (`:338`, `:341`)
fail (a). Sheets refuses Freeze, Sort range, Filter views, Data validation and
Conditional formatting a seat on exactly clause (c), which is why its toolbar
has stayed one row wide for fifteen years while Excel's Home tab has not.

**What the rule promotes that has no toolbar route today:** Print, Insert link,
Insert comment, Insert chart — all four act on the selection immediately and all
four are undoable.

**Adding a control next year.** Run the four tests. If it passes, **the bar is
full**: the change must name the control it displaces and move that one to
`⌄ More` or to a menu in the same diff. If it fails, it does not go on the bar,
and "but it is important" is answered by `Alt+/`. That is what buys the right to
a thirty-control bar. Restraint without a search field is amputation, and a
product that amputates puts the control back next year and grows a ribbon.

**And the mechanical rule that overrides all of the above:** a new control is a
**proxy**, never a new id (§5.1), and it carries no `id` starting `tb-`.

### 6.2 The row, in order

`┃` is a 1 px hairline in a 6 + 1 + 6 budget (`webapp/editor.css:879-884`).

| # | slot | id | kind | group | status |
| --- | --- | --- | --- | --- | --- |
| 1 | Undo | `toolbar.undo` | button | history | live |
| 2 | Redo | `toolbar.redo` | button | history | live |
| 3 | Print | `file.print` | button | history | **proxy — new node** |
| 4 | Paint format | `toolbar.painter` | toggle | history | live |
| ┃ | | | | | |
| 5 | Format as currency | `toolbar.currency` | button | number | live |
| 6 | Format as percent | `toolbar.percent` | button | number | live |
| 7 | Decrease decimal places | `toolbar.dec-dec` | button | number | live |
| 8 | Increase decimal places | `toolbar.inc-dec` | button | number | live |
| 9 | **`123 ▾`** | `toolbar.numfmt` | dropdown | number | live |
| ┃ | | | | | |
| 10 | Font | `toolbar.font` | combo | font | live |
| ┃ | | | | | |
| 11 | `−` | `toolbar.size-down` | button | font-size | live, reglyphed |
| 12 | Font size | `toolbar.size` | combo | font-size | live |
| 13 | `+` | `toolbar.size-up` | button | font-size | live, reglyphed |
| ┃ | | | | | |
| 14 | Bold | `toolbar.bold` | toggle | text | live |
| 15 | Italic | `toolbar.italic` | toggle | text | live |
| 16 | Underline | `toolbar.underline` | toggle | text | live |
| 17 | Strikethrough | `toolbar.strike` | toggle | text | live |
| 18 | Text colour | `toolbar.fontcolor` | split | text | live |
| ┃ | | | | | |
| 19 | Fill colour | `toolbar.fillcolor` | split | cell | live |
| 20 | Borders | `toolbar.border` | split | cell | live |
| 21 | Merge cells | `toolbar.merge` | split | cell | live |
| ┃ | | | | | |
| 22 | Horizontal align | `format.alignment.left` / `.center` / `.right` | dropdown | align | **proxy — new node** |
| 23 | Vertical align | `toolbar.valign` | dropdown | align | live |
| 24 | Text wrapping | `toolbar.wrap` | dropdown | align | live |
| 25 | Text rotation | `toolbar.rotate` | dropdown | align | live |
| ┃ | | | | | |
| 26 | Insert link | `insert.hyperlink` | button | insert | **proxy — new node** |
| 27 | Add comment | `insert.note` | button | insert | **proxy — new node** |
| 28 | Insert chart | `insert.chart-auto` | button | insert | **to mint** |
| 29 | Create a filter | `toolbar.filter` | toggle | insert | live |
| 30 | Functions | `insert.function` | split | insert | **to mint** |
| | More | `toolbar.more` | overflow | — | live |

### 6.3 Per-control notes the table has no room for

**`123 ▾` keeps its label where everything else is a glyph.** It is the control
that answers *what format is this cell in?*, it already carries a live readout
(`#tb-numfmt-label`, `webapp/editor.html:402`), and it is the one toolbar button
that already draws its caret (`:401`). `docs/88:295` cites Sheets' `123 ⌄` as
the fix for our bare `#` glyph. **56 px**, not a 96 px format-name readout: the
menu already ticks the active format (`#numfmt-menu button.checked::before`,
`webapp/editor.css:1497-1500`), so the name does not need permanent space.

**The `− size +` triad is Sheets' signature.** `A▲` / `A▼`
(`webapp/editor.html:276-277`) reglyph to plain `±`; the A-glyphs are Excel's.

**Text colour, fill colour and borders become true splits** — the body
re-applies the last-used value, a 14 px caret zone opens the picker — because
re-applying the same colour is the common case. They are also **the only three
glyphs in the chrome allowed to carry colour**. The fill-colour picker gains a
*Conditional formatting* shortcut at its foot, which is Sheets' bridge from
colouring one cell to writing a rule. Merge and Σ are splits for the same
reason; align, vertical align, wrap, rotation and `123 ▾` are **not**, because
there is no sensible last-used default.

**Text wrapping needs no work at all.** `#tb-wrap` (`webapp/editor.html:345`) is
already the three named states Overflow / Wrap / Clip, which is Sheets' model
and better than Excel's toggle-plus-a-dialog-checkbox.

**Horizontal align is one dropdown replacing three bare buttons, and that is a
governance fix as much as a shape one.** The three `.tb-align` buttons
(`webapp/editor.html:316`, `:319`, `:322`) carry `data-al` and **no `id`**, so
they mint no command and sit outside `listCommands()`, `applyCommandRules()`,
the read-only whitelist and the capability gates. Proxying the three
`format.alignment` ids puts them inside the cascade for the first time, at no id
cost.

**Functions is the highest-value gap on the bar.** Its body is `autoSum()`
(`webapp/editor.selection.js:1320`), which has **no DOM node at all** and is
reachable only by chord; its "All functions…" proxies `#fx-insert`
(`webapp/editor.html:502`), which also has no id. Two ids take a verb from
*exists, unreachable and ungoverned* to *in `listCommands()`, in the OS menu,
inside `applyCommandRules()`*.

**Insert chart is argument-free on purpose.** It detects the block around the
selection (`session_block_bounds`, already used at `webapp/editor.core.js:4640`),
infers a kind — column for one numeric series against labels, line when the
first column parses as dates, scatter for two numeric columns — anchors clear of
the source data, and opens the chart panel. The one decision that most needs
real data in front of it stops being made from a blind submenu. It needs **one**
new id, because the seven `insert.chart.<kind>` ids are already taken by the
submenu, and that id is chosen jointly with the ribbon workflow.

**Underline is a deliberate divergence.** Sheets demotes it to its Format menu
and promotes Strikethrough instead. We keep both: our instrument is frequency,
Underline passes it as plainly as Bold does, and Sheets' omission is a
Docs-inherited artefact rather than a frequency judgement. Copying it would be
copying the pixels instead of the rule.

**Text rotation is the most marginal survivor on clause (a)** and is kept only
because it is a single dropdown that already exists and Sheets keeps it too. If
the bar ever needs a slot back, it is the first to spend.

**Cut / Copy / Paste stay off**, in both products. They fail (d): every keyboard
has them and the context menu draws them (`webapp/editor.dialogs.js:2476-2478`).
`docs/88:332` records that Sheets carries none of them.

### 6.4 The overflow, and the collapse ladder

**Two populations in one `⌄` chevron, and the distinction is the whole design.**

**Permanent residents** — nodes that must stay in the DOM but fail the selection
rule: `toolbar.freeze` (c), `toolbar.sort` (c), `toolbar.comma` (a)+(d),
`toolbar.indent-more` and `toolbar.indent-less` (a). Five controls, always
there, at the top of the flyout in their original group order with their
hairlines intact — so the chevron is never empty and never a surprise.

**Then, as the window narrows — and here we deliberately do *not* copy Sheets.**
Sheets collapses right-to-left one anonymous control at a time into one
unlabelled chevron, and the documented consequence is users concluding the
feature was removed (https://support.google.com/docs/thread/174270123 and
https://support.google.com/docs/thread/410076953, accessed 2026-09-09). Our
machinery folds whole **labelled** groups in a declared priority order
(`reflowToolbar()`, `webapp/editor.core.js:9402`; collapsibles gathered at
`:9348` from `.tb-group[data-collapse]`; chips at `webapp/editor.html:279`,
`:312`, `:377`, `:432`, `:463`), producing findable `Insert ⌄` / `Cell ⌄` chips
instead of a void. **Adopt Sheets' restraint about what is on the bar; keep our
degradation of what happens when it does not fit.**

**The width budget** — computed from §8's metrics, not observed:

```
  history   undo redo print painter        4×28 + 3×4 =  124
+ number    $ % .0← .00→ 123▾              4×28 + 56 + 4×4 = 184
+ font      combo                                     =  140
+ font-size − field +                      28+48+28 + 2×4 = 112
+ text      B I U S + colour split         4×28 + 42 + 4×4 = 170
+ cell      fill borders merge (3 splits)  3×42 + 2×4 =  134
+ align     h v wrap rotation              4×28 + 3×4 =  124
+ insert    link comment chart filter Σ    4×28 + 42 + 4×4 = 170
= 1158 px of controls
+ 7 group boundaries × 13                  =   91
+ container padding 6 + 6                  =   12
+ band inset 8 + 8                         =   16
+ the ⌄ chevron (permanently resident)     =   28
= 1305 px required
```

**Against our current bar's 1463 px (`docs/88:32`) and the ribbon's 1390 px
Simplified budget (`docs/91` §5.2.)** 158 px less than what ships and 85 px less
than the ribbon — the editorial restraint paying for itself in width as well as
in ink.

**The authored ladder.** There is no `fits()` loop. Each step is a named width,
so a command never changes position between two widths — it only ever falls off
the end, which is the invariant that makes the row learnable.

| viewport | step | what folds | required after |
| --- | --- | --- | --- |
| **≥ 1366** | — | full row | 1305 (61 px slack at 1366; 135 at 1440) |
| 1280–1365 | 1 | insert → `Insert ⌄` | 1207 |
| 1180–1279 | 2 | + cell → `Cell ⌄` | 1145 |
| 1120–1179 | 3 | + align → `Align ⌄` | 1093 |
| 1024–1119 | 4 | + size triad → the field alone; font combo 140 → 96 | 985 |
| 900–1023 | 5 | + text tail (U, S, text colour) → `Text ⌄`; Paint format → `⌄` | 855 |
| 768–899 | 6 | + number quick-set ($ % .0← .00→) behind `123 ▾`; font → `Font ⌄` | 659 |
| ≤ 767 | 7 | every group is a labelled chip; the container becomes its own `overflow-x` scroller with the `⌄` pinned **outside** it | — |

**Six controls never enter a chip at any width ≥ 320 px:** `toolbar.undo`,
`toolbar.redo`, `toolbar.numfmt`, `toolbar.bold`, `toolbar.italic`,
`toolbar.more`. Groups collapse *around* them — at 900 the text group folds to
`Text ⌄` carrying U, S and text colour while B and I stay flush on the row —
which is why the row's left half is stable at every width.

**Why this does not reproduce the 1461 px defect.** (1) The first collapse is at
1280, a width somebody chose, and both mainstream laptop widths carry headroom:
135 px at 1440, 61 px at 1366. The defect was never that a collapse existed — it
was that the first one fired above every mainstream laptop because nobody had
picked a number. (2) There is no measurement loop, so there is no width at which
the row silently reorders; between 1440 and 1366 the row is byte-identical.
(3) The collapse unit is a labelled chip, not an anonymous glyph. (4) On a
desktop, `.tb-collapsed` chips disappear entirely at 1366 and above.

**No animation on reflow.** Controls entering or leaving a chip or the flyout
appear and disappear instantly; only the container width reflows. Animating a
resize-driven reflow produces jitter for the whole duration of a window drag.

**Three mechanical constraints.** (1) A control parked in a closed flyout must
not become a stray tab stop — `syncStops()` already resets `tabIndex = -1`
across the toolbar including parked nodes (`webapp/editor.core.js:9453`) and
`items()` filters on `offsetParent !== null` (`:9441`), but the permanent
residents multiply the parked count, so the trap is re-tested rather than
assumed. (2) A click inside the flyout must not dismiss it — the carve-out at
`webapp/editor.core.js:9430` — and the flyout now holds live menus (freeze,
sort). (3) Nothing in the flyout may be `.oc-cmd-hidden`; `hidden` inside a
closed popover is the correct state and needs its own `[hidden] { display: none }`
guard, as `.tb-collapsed[hidden]` (`webapp/editor.css:375`), `.tb-group[hidden]`
(`:377`), `.menu-top[hidden]` (`:541`) and `.popmenu[hidden]` (`:856`) already
have. The comment at `:535-540` records that this trap has been sprung **four
times**.

---

## 7. The panels

### 7.1 The panel contract

One `<aside role="complementary">` docked right — **the only one**. Opening a
second tool re-targets it rather than stacking; this is already how
`openPanel(tool)` behaves (`webapp/editor.core.js:5145`, body cleared at
`:5160-5161`).

- **Width** `clamp(280px, 24vw, 400px)`, default 320, user-draggable by a 4 px
  hit target on its left border, persisted per tool. Today it is a rigid
  `flex: 0 0 316px` (`webapp/editor.css:1301`) at every viewport, and **no
  `@media` rule in the file mentions `.side-panel`** — the media queries are at
  `:172`, `:431`, `:527`, `:1124`, `:1153`, `:1246`, `:1505`, `:1886`, `:1896`,
  `:2181`.
- **Not modal**, above 1024 px: no scrim, no focus trap, no `aria-modal`. The
  grid keeps its selection, keeps accepting clicks, and every panel field that
  names a range accepts a click-drag on the grid to re-target it. That is the
  whole point — *"Sidebars do not suspend the server-side script while the
  dialog is open"* (https://developers.google.com/apps-script/guides/dialogs,
  accessed 2026-09-09) is Google's own statement of the distinction, and the
  practical consequence is that a chart's type, a conditional format's threshold
  and a validation rule's range are chosen against live data rather than from
  memory.
- **The grid reflows; the panel does not overlay it.** §9 item 2 specifies how,
  and why `resize()` runs exactly once.
- **Escape is scoped.** §11.
- **Applies on change, not on an Apply button**, with a snackbar carrying Undo.
- **Focus** moves to the panel's first control on open and returns to the
  **invoker** on close — not unconditionally to the canvas, which is what
  `closePanel()` does today (`webapp/editor.core.js:5463`+).

### 7.2 The panels

`replaces` names the modal that retires. `reaches` names a `docs/92` §5 row that
becomes reachable in the same act.

| panel | opens from | contents | replaces / reaches | status |
| --- | --- | --- | --- | --- |
| **Chart editor** | toolbar chart button · Insert ▸ Chart ▸ ⟨kind⟩ · double-click a chart | Two tabs, **Setup** and **Customize**, over `buildChartPanel` (`webapp/editor.core.js:4417-4490`). Setup: Chart type (**already built**, `:4429-4437`), Data range with a grid-picker chip, Series, Switch rows/columns. Customize: titles (`:4453-4455`), Legend (`:4457-4468`), and accordion sections for stacking/grouping, secondary axis, data labels | replaces `chartDialog(kind)`'s type-first submenu and the `confirmModal("Delete chart", …)` at `:4535`, which asks before an operation the engine proves reversible (`crates/casual-calc-wasm/src/lib.rs:2189`). **Reaches `docs/92` R-3** — `ChartGrouping` at `crates/casual-calc-model/src/chart.rs:56-75`, wire fields already accepted at `crates/casual-calc-wasm/src/objects.rs:98`/`:130`/`:133`. **Trap:** send *grouping*, not a synthetic kind — `retunable_in_place` (`objects.rs:591-604`) retunes grouping in place, a *kind* change detaches the retained part | exists |
| **Pivot table editor** | Insert ▸ PivotTable… · Data ▸ PivotTable fields… | Four wells — Rows, Columns, Values, Filters — each with Add; placed fields expand inline with order, sort, Summarise by, Show as (`webapp/editor.pivot.js`; entry at `webapp/editor.core.js:4849`+) | replaces `confirmModal("Delete pivot table", …)` at `:4815`. **Ours already exceeds Sheets' here and must not be trimmed toward it** (§13) | exists |
| **Conditional format rules** | Format ▸ Conditional formatting… (edit state) · Format ▸ Conditional formatting rules… (list state) · the foot of the colour pickers | **One panel, two states.** *List:* every rule in evaluation order, each row a swatch, the range in mono and a one-line description; hovering paints a 2 px dashed outline over that rule's range through the `perQuad` overlay already used for collaborator ranges (`webapp/editor.paint.js:891-901`); drag to reorder, because rules evaluate in list order; a row `⋮` carries Duplicate, Delete, stop-if-true. *Edit:* today's `buildCfPanel` (`webapp/editor.dialogs.js:1528`) with a back-chevron | replaces `manageCfRules()` (`:89-138`) — a blocking modal that lists rules, cannot edit them, and covers the cells it is describing, reached from `webapp/editor.core.js:9837`, **44 lines** from its panel twin at `:9881` with a label differing by one word. **Reaches `docs/92` R-8** — `NotEqualTo`, `NotBetween`, `GreaterThanOrEqual`, `LessThanOrEqual` all exist and evaluate in `CfRule` (`crates/casual-calc-model/src/sheet.rs:914-921`) and are absent from the operator array at `webapp/editor.dialogs.js:1531-1546`, so a workbook opened and edited here **silently narrows** | exists |
| **Data validation rules** | Data ▸ Data validation… · Insert ▸ Dropdown | A **list** of every rule on the sheet with *Add rule*; each expands to Apply to range, Criteria, values editor, and Advanced options carrying the display style and warn-or-reject | none — this was already a panel (`buildDvPanel`, `webapp/editor.dialogs.js:1332`), but it edits one rule at a time and cannot enumerate. Sheets converted its own from a modal in 2022; we were there first and stopped short | exists |
| **Named ranges** | Data ▸ Named ranges · Tools ▸ Name manager… · the Name Box caret | Every defined name with its A1 reference, edit and delete per row, *Add a range* at the foot, naming rules enforced inline | replaces `openNameManager(x, y)` (`webapp/editor.dialogs.js:2244`) — an anchored modal positioned by coordinates the caller passes (`webapp/editor.core.js:9934` passes `160, 120`), which is a dialog pretending to be a popover | **gap** |
| **Protected sheets and ranges** | Data ▸ Protected sheets and ranges · a sheet tab's caret | A list of existing protections, *Add a sheet or range*, a Range / Sheet pair, the permission choice | none — today the whole feature is three menu ticks under Format ▸ Protection (`webapp/editor.core.js:9828-9835`) with **no surface that answers "what is protected in this workbook"**. The three ids stay exactly where they are | **gap** |
| **Version history** | the app bar's "Last edit was …" · File ▸ Version history · Ctrl+Alt+Shift+H | Reverse-chronological versions from `session_versions()` (`webapp/editor.core.js:5213`), each with a per-author colour swatch — `cellAuthor()` over `session_cell_author` already exists (`:5203-5206`), so attribution is wiring, not engine work. **Add:** *Name this version* on the per-version `⋮`, an *Only show named versions* toggle, and Sheets' *Show unmodified rows* review mode | none — `buildHistoryPanel` (`:5208`) is already a panel. **Bound this must not overstate:** a version here is a snapshot, not a replayed log ([83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md)) | exists |
| **Comments** | toolbar button · Insert ▸ Comment (Ctrl+Alt+M) · View ▸ Comments (Ctrl+Alt+Shift+A) · the app bar's comment-history icon | Per-cell as now (`buildNotePanel`, `webapp/editor.core.js:5534`), **plus a list mode**: every open thread, newest first, anchor cell + author + first line + reply count, clicking scrolls the grid there. @mention with an *Assign to ⟨name⟩* checkbox, reactions, Resolve collapsing into a *Show resolved* disclosure. **And the name field goes** — attribute from `participantName` over `collabRoster` | replaces the `"Your name (optional)"` input at `:5545-5549` (`commentAuthor()`/`setCommentAuthor()`, `webapp/editor.sheets.js:498-504`) while the roster's name for that same person is already painted on their live cursor (`webapp/editor.paint.js:904`). One person, two identities, one document | exists |
| **Cell format** | Format ▸ Cell format… · Ctrl+1 · right-click ▸ Format cells… | The seven OOXML facets as accordion sections rather than a tabbed dialog: Number, Font, Alignment, Border, Fill, Protection | replaces `formatCellsDialog()` (`webapp/editor.dialogs.js:141`). **Sheets has no equivalent surface at all** — every one of Excel's tabs is a picker or a Format submenu there — and we cannot delete it, because fidelity is a stated bound and the file carries states the toolbar cannot author. **Reaches `docs/92` R-14** — shrink-to-fit has a wire byte at `crates/casual-calc-wasm/src/axis.rs:815-817` and a renderer at `webapp/editor.core.js:3014-3020` and no control anywhere; needs one setter beside `session_set_rotation` (`crates/casual-calc-wasm/src/structural.rs:696`) and one checkbox | **gap** |
| **Sort range** | Data ▸ Sort range ▸ Custom sort… · the sort control in `⌄` | A stack of sort keys with *Add another sort column*, the affected range outlined in the grid while the panel is open | replaces `sortDialog()` (`webapp/editor.dialogs.js:590`) — a blocking modal covering the rows it is about to reorder. **Reaches `docs/92` C-8** — the three-key cap is a literal at `:641`; `session_sort_range_multi` already takes a vector (`crates/casual-calc-wasm/src/structural.rs:415-423`). Colour and custom-list keys are a **different, larger** row and must not be scheduled with this one | **gap** |
| **Split text to columns** | Data ▸ Text to columns… | Delimiter choice with a **live preview of the split against the actual selection** — which is the whole reason it should not be a modal, since the delimiter guess is a hypothesis about data that is behind the dialog | replaces `textToColumnsDialog()` (`webapp/editor.dialogs.js:2159`) | **gap** |
| **Table** | Insert ▸ Table… (Ctrl+T) · Format ▸ Convert to table… | Range, headers, totals row, banding, style — today's `buildTablePanel` (`webapp/editor.dialogs.js:1060`) | replaces `tableDialog()` (`:1867`). The panel exists; a route to it that is not a modal first does not | exists |
| **Page setup** | File ▸ Page setup… | Everything OOXML records about printing a sheet, applied on change with one `session_set_page_setup` call each so every switch is its own undo step (`buildPagePanel`, `webapp/editor.dialogs.js:1195`) | ours, placed deliberately — Sheets has no page-oriented view at all. **Reaches `docs/92` R-6** once one wasm export returns page boundaries: `Page` and `Plan` exist at `crates/casual-calc-layout/src/print.rs:1033-1070` and the draw loop is already at `webapp/editor.core.js:2535-2562` | exists |
| **Column stats** | Data ▸ Column stats… | Count, blanks and the type distribution — how you find the one text cell wrecking a `SUM` (`buildStatsPanel`, entry at `webapp/editor.core.js:9900`) | ours **and** Sheets' | exists |
| **Import report** | the app-bar chip · File ▸ Import report… | Everything the model could not keep, counted and named — sheets, defined names, functions, features — with a jump-to-cell for anything anchored | **a compliance surface, not a nicety.** `CLAUDE.md`'s baseline is that anything the model cannot keep is "counted and named in a compatibility report, never dropped quietly", and today `reportImportIssues()` delivers that promise by appending a `<span>` to `#tb-status` (`webapp/editor.dialogs.js:2075-2087`) — the same 34 px strip that also says "loading engine…" and carries every error the editor produces. The summary is correctly escaped and correctly generated; it has no home | **gap** |
| **Share** | the app-bar pill · File ▸ Share… | `shareDialog()`, opened as a panel | Sheets keeps Share as one of its few genuine modals; ours becomes a panel because the permission list is read against the document | exists |

### 7.3 Three surfaces that are deliberately not panels

**Insert link — an anchored bubble.** A link is configured against the cell it
will live in and there is nothing else to look at; Sheets anchors it beside the
cell. Replaces `hyperlinkDialog()` (`webapp/editor.dialogs.js:1788`).

> **Narrow fix first, independently of this chrome.** That dialog's two buttons
> are 21 px tall because they are the only buttons in `webapp/` built without
> the `oc-btn` class — `el("button", null, "Cancel")` at
> `webapp/editor.dialogs.js:1832` and `el("button", "primary", …)` at `:1834`,
> where the bare `primary` class matches no rule (`.oc-btn.primary` at
> `webapp/editor.css:616` is the only one, and `.oc-btn` is what supplies
> `padding: 7px 14px` at `:582-584`). That is `docs/82:21-22`, a live finding
> against a standing 24 px floor, and it is a two-token change that should not
> wait on a chrome.

**Filter view mode — chrome, not a panel. The most transferable Sheets idiom.**
A dark banner slides down above the column headers carrying the view's name and
range as editable fields, a `⚙` menu and an `✕`; the header bands cross-fade
dark for as long as the view is active. **The chrome itself says "you are in a
state, other people are not affected, and here is the exit"**, which is exactly
what a shared workbook's invisible filter state fails to communicate. We already
have the substance — `data.clear-my-view` (`webapp/editor.core.js:9899`) and
[71](71-FILTER-SHARING-AND-VIEWS.md) — and none of the signalling. Because it is
a *mode* change rather than a panel it earns the longest transition in the
chrome (§9 item 11).

**Snackbar — the surface that lets every panel stay non-blocking.** 48 px tall,
288–568 px wide, 24 px from the **grid viewport's** left and bottom edges so it
never covers the tab strip or `#tb-status` (`.grid-wrap` is already
`position: relative`, `webapp/editor.css:1298`). One action, labelled with the
verb it reverses, never "Dismiss". Undo dispatches `doUndo()`
(`webapp/editor.selection.js:1383`).

**The eligibility rule is selection-rule clause (b) again:** routes through
`EditOperation` ⇒ snackbar; loss outside the model ⇒ confirm. `confirmLoss()`
(`webapp/editor.sheets.js:640-648`) and File ▸ New's discard prompt are correct
confirms and stay exactly as they are. The two chart/pivot delete confirms are
not, and they go. It also replaces the ~37 sites that report a *completed*
action by writing into `#tb-status`, roughly 700 px from where it happened.

**Auto-dismiss at 6 s and immediately on any local edit.** That second clause is
a correctness requirement, not polish: undo is per-user and travels to peers
([69](69-COLLABORATIVE-UNDO-POLICY.md)), so a stale Undo button would reverse
the wrong operation.

### 7.4 Tool finder

A 480 px popover filtering `menuModel()` (`webapp/editor.selection.js:1065`),
which already carries every leaf's id, label, enabled state and menu path
**derived from the live DOM** — so read-only mode and host capability rules are
honoured for free and the list cannot drift from the menus. Each result shows
the label with its path in muted type (`Format ▸ Number ▸ Custom…`) and the
accelerator where one is declared; Enter dispatches `runCommand(id)` (`:1146`);
the empty state is the six most recently run commands, persisted per document.

**Disabled commands are shown greyed with the reason, not hidden** — *"I cannot
find it"* and *"I cannot use it here"* are different problems, and only one of
them is solved by omission.

`docs/92` X-8 rates this *small* precisely because the registry is already
public SDK surface. It is the panel that makes the whole chrome safe.

---

## 8. Control metrics, and every state

### 8.1 Sizes

| control | box | icon | source |
| --- | --- | --- | --- |
| Toolbar icon button | **28 × 28**, radius 3 | 16 | `webapp/editor.css:763`; the flat floor asserted at `tests/browser/editor.native-chrome.spec.mjs:408-409` |
| App-bar icon button (`.tb-icon`) | **32 × 32**, radius 8 | 16 | `webapp/editor.css:779-780` |
| Split button | **28 + 14 = 42**, one radius-3 box | 16 + 8×5 caret | the caret half is `docs/91` §2.1's 14 px, kept identical so the two chromes do not diverge on the same control |
| Combo — font name | 28 × **140** (→ 96 at step 4) | — | `docs/91` §2.1 |
| Combo — font size | 28 × **48**, `tabular-nums` | — | with `−`/`+` at 28 each |
| Number format `123 ▾` | 28 × **56**, 12.5 px `tabular-nums` | 8×5 caret | §6.3 |
| Share pill | **28** × min 84, radius 999 | 16 | §8.3 |
| Presence face | **18 × 18** circle, `margin-left: −5px`, 1.5 px ring | — | unchanged, `webapp/editor.css:463-472` |
| Presence button | 22 tall, radius 999, 12/600 | — | unchanged, `webapp/editor.css:450-455` |
| Menu-bar label | 24 tall, `padding: 0 10px`, radius 6, 13 px | — | `.menu-top`, `webapp/editor.css:530-534` |
| Menu row | 25 mouse / 44 coarse, radius 6, 13/500 | 16 | `webapp/editor.css:864-877`, coarse block at `:2181`+ |
| Sheet tab | 26 tall, radius 6, 12.5/600 | — | unchanged, `webapp/editor.css:1046-1051`; the one named exception to the 28 floor (`tests/browser/editor.native-chrome.spec.mjs:399-401`) |
| Snackbar | 48 tall, 288–568 wide, radius 8 | — | M1 snackbar geometry, https://m1.material.io/components/snackbars-toasts.html (accessed 2026-09-09) |
| Side panel | `clamp(280px, 24vw, 400px)`, default **320** | — | today's `flex: 0 0 316px` (`webapp/editor.css:1301`) rounded onto the 8 px grid |

**Per-button pitch is 32 px** — 28 box + 4 gap. That is exactly the figure
`docs/88:128` measures inside our own groups *and* attributes to Sheets, so the
flattened row costs the same per control as the bar it replaces while carrying
six fewer controls. Today's `.toolbar { gap: 6px }` (`webapp/editor.css:355`)
drops to 4, which is ~50 px of width back across the row.

### 8.2 Radii, spacing and separators — three values, no new scale

**3 px** on every toolbar control ≤ 32 px (`webapp/editor.css:763`, whose
comment argues 8 px reads as a pill). **6 px** on menu rows (`:874`), flyout
rows (`:394`) and menu-bar labels (`:533`). **8 px** on `.tb-icon` (`:779`) and
on the snackbar — the house's existing surface-of-a-control radius rather than a
fourth value. **12 px** on menus, popovers and colour pickers (`:850-852`).
**16 px** on the toolbar container and **999** on the Share pill and the
presence button (`:454`). **0 on the side panel**, which is in flow with a
hairline left edge (`:1302`) and no shadow, because it is attached to the page
rather than floating above it.

**`.tb-sep`** (`webapp/editor.css:884`): 1 px wide, **16 px tall** (was 20),
`--oc-border-color`, `margin: 0`, in the **6 + 1 + 6 = 13 px** budget the file's
own comment at `:879-883` defends. 16/32 preserves the 50 % rule-to-container
ratio the 20 px rule had against a 41 px bar. Seven boundaries × 13 = **91 px**
of the row spent on grouping. **Do not widen to Sheets' 8 + 1 + 8**, which would
cost 28 px more and buy nothing at 1440. The rules are the only statement of
grouping there is: no capsules, no fills, no captions (`docs/88:254`). Add
`role="separator" aria-orientation="vertical"`; `.tb-sep` carries no role today.

### 8.3 Tonal surfaces, and the inversion

| level | light | dark | token | carried by |
| --- | --- | --- | --- | --- |
| chrome ground | `#f6f7f9` | `#1a1f27` | `--oc-chrome-ground` → `--oc-surface-color` (`:80`/`:139`) | app bar, menu bar, toolbar band |
| container | `#ffffff` | `#0f1216` | `--oc-chrome-container` → `--oc-background-color` (`:71`/`:130`) | the toolbar's rounded strip |
| canvas | `#ffffff` | `#0f1216` | `--oc-background-color` | formula bar, grid, bottom bar |
| overlay | `#ffffff` | `#171b21` | `--oc-popover-background-color` (`:81`/`:140`) | menus, popovers, the panel |

**Zero new colour tokens.** The container and the canvas are the same colour;
the container is distinguished by **elevation and radius**, not by tone. That is
Sheets' "elevation and tone instead of borders" stated precisely, and it means a
host that overrides two tokens still gets a coherent chrome.

**We invert Sheets' tonal direction by one level, deliberately.** Sheets goes
white grid < light header < *more* tonal container. We go white grid < tonal
ground < white container, because that is the house idiom already in the
stylesheet: **a pill on a bar lifts, a row in a panel tints**
(`webapp/editor.css:767` against `:877`). The Sheets chrome moves the lift up
one level — **the container lifts once, the controls inside it tint and never
lift** — so there is exactly one elevation in the header block instead of one
per hovered button.

**The tonal step is 1.03:1**, far below any contrast threshold, which is why it
is never the only signal: the container also carries elevation and radius. Under
`prefers-contrast: more` (`webapp/editor.css:1886-1889`, which lifts
`--oc-border-color`) the container gains a 1 px `--oc-border-color` outline.

**Elevation: two, and only two.** `--oc-elevation-raised`
(`webapp/editor.css:117`) on the toolbar container. `--oc-elevation-overlay`
(`:118`) on menus, popovers and the snackbar. Nothing else in the chrome has a
shadow. If the container lifts and the buttons inside it also lift, the
container stops reading as a surface.

**The toolbar container.** Height 32, radius **16** (half the height, so it
reads as terminated rather than merely rounded), horizontal padding 6, inset
8 px from each band edge (matching `.toolbar { padding: 0 8px }`,
`webapp/editor.css:357`), fill `--oc-chrome-container`, **no border**, content
left-aligned with the `⌄` pinned right by `margin-left: auto`.
`docs/88:155-159` quotes Material 3 against rounding a toolbar container; that
guidance is about the full-bleed bar, and the complaint it records was against a
row of *filled group capsules*. Sheets obeys its spirit by rounding exactly one
container and leaving the buttons inside it unfilled until hovered. **Round the
strip or round the groups, never both.**

**The Share pill.** Height 28, min-width 84, `padding: 0 14px 0 10px`, gap 8,
radius 999, 16 px glyph, 13/500 label. Fill `--oc-accent-soft`
(`webapp/editor.css:120`), label and glyph `--oc-accent-hover-color`.
**Not `--oc-accent-color` on that fill:** `#2f6df6` on the resolved `#dbe4fd`
computes to **3.57:1** and fails; `#1f56cf` computes to **5.01:1** and passes.
The pair inverts correctly in dark by construction, because
`--oc-accent-hover-color` is *lighter* than the accent there (`#7aa0ff`,
`:143`). **Both figures are calculated, not measured** — the gate must measure
them the way `tests/browser/editor.chrome-and-contrast.spec.mjs` measures
gridlines. Sheets' Share pill is a tonal green container with a dark green
label; we reproduce the **structure** in the one host-swappable brand token,
per `ADR-026`'s accent rule. **No Google green, ever.**

### 8.4 New tokens

Declared in **all three** blocks — `:70`, `:128`, `:172` — or the OS-dark path
or the manual choice breaks. **Not added to `sdk/theme-tokens.json`**; that is a
separate, deliberate act.

```
--oc-chrome-ground:        var(--oc-surface-color)
--oc-chrome-container:     var(--oc-background-color)
--oc-chrome-toolbar-inset: 4px    /* 0 collapses the container to a flat 34px band */
--oc-pressed-color:        color-mix(in srgb, var(--oc-text-color) 9%, var(--oc-background-color))
--oc-accent-soft-hover:    color-mix(in srgb, var(--oc-accent-color) 26%, transparent)
```

plus §9's motion tokens. **Two already-published tokens finally get real
users:** `--oc-accent-ring` (`webapp/editor.css:119` — `docs/91` cites `:118`
and `:110`; both are wrong, `:118` is `--oc-elevation-overlay` and `:110` is a
comment line) becomes the focus ring everywhere, and `--oc-accent-soft`
(`:120`), used today only by `::selection` (`:226`), becomes the Share pill and
every toggled control.

> **`--oc-pressed-color` conflicts with `docs/91` §2.2**, which defines it as
> `var(--oc-surface-color)`. That value **is** this chrome's hover fill, so a
> press would be invisible here. The derived value above darkens correctly
> against both chromes' hover fills. **One token, one value, both chromes —
> resolve it before either lands** (§14 R-7).

### 8.5 Every state, per control kind

`--oc-pressed` is shorthand for `--oc-pressed-color`.

**Toolbar icon button (28 × 28, radius 3)**

| state | background | glyph | elevation | outline |
| --- | --- | --- | --- | --- |
| rest | transparent | `--oc-icon-color` | none | none |
| hover | `--oc-surface-color` | `--oc-text-color` | **none** — flat tint, not a lift | none |
| pressed | `--oc-pressed` | `--oc-text-color` | none | none |
| focus-visible | as underlying state | as state | as state | `2px solid --oc-accent-color` offset 1, **plus** `0 0 0 4px --oc-accent-ring`, **`transition: none`** |
| active (checked) | `--oc-accent-soft` | `--oc-accent-color` | none | none |
| active + hover | `--oc-accent-soft-hover` | `--oc-accent-color` | none | none |
| active + pressed | `color-mix(in srgb, var(--oc-accent-color) 34%, transparent)` | `--oc-accent-color` | none | none |
| **mixed** (`aria-pressed="mixed"`) | transparent | `--oc-accent-color` | `inset 0 0 0 1px --oc-border-color`, opacity .75 | none |
| disabled | transparent | `--oc-disabled-color` | none | `cursor: default`, **the `disabled` attribute** |

Rest and disabled are exactly what ships (`webapp/editor.css:763`, `:768`).
**Hover and checked change**: today hover is `--oc-background-color` +
`--oc-control-shadow` (`:767`) and checked is the same fill with an accent glyph
(`:769-771`). Inside a white container neither can lift, so both go tonal. The
mixed row is unchanged (`:1487-1493`) and is one of two things this stylesheet
already does better than Sheets. **Checked additionally swaps to the filled icon
variant**, so the state survives greyscale and `prefers-contrast: more`.

**Toggle** — identical; `aria-pressed` carries the state, `role="switch"` does
**not**: a formatting toggle is a command with a latch, not a setting. Paint
format has two latch depths (single click one-shot, double click sticky); both
report `aria-pressed="true"` and the tooltip names which.

**Split button (28 + 14, one radius-3 box, two real `<button>`s in a
`role="group"`)**

| state | primary half | caret half |
| --- | --- | --- |
| rest | transparent, `--oc-icon-color` | transparent, `--oc-icon-color` at 70 % |
| hover on primary | `--oc-surface-color`, **and a 1 px `--oc-border-color` hairline appears between the halves** | unchanged |
| hover on caret | unchanged | `--oc-surface-color`, same hairline |
| pressed | `--oc-pressed`, that half only | that half only |
| menu open | rest | `--oc-surface-color`, `aria-expanded="true"`, caret rotates 180° over `--oc-dur-2` |
| active (latched primary) | `--oc-accent-soft` + `--oc-accent-color` | unchanged |
| focus-visible | ring on the focused half at `outline-offset: -1px`, so the two never touch | as left |
| disabled | **both halves together** — a caret opening a menu of dead commands is worse than no caret | as left |

**The disclosure caret, as a hard invariant.** One rule —
`.tb-btn[aria-haspopup="true"]::after` — renders an 8 × 5 chevron in
`currentColor` at 70 % opacity with 4 px of leading space, so **the visual
signal is derived from the ARIA semantics and cannot drift from it**. The gate
asserts the biconditional in both directions (§16).

**Combo box (font name, font size, `123 ▾`)**

| state | field | outline |
| --- | --- | --- |
| rest | transparent | 1 px `--oc-control-border-color`, **bottom edge only** |
| hover | transparent | full 1 px `--oc-border-hover-color` |
| focus-visible | transparent | `2px solid --oc-accent-color`, `outline-offset: -1px` — **inset, so the field does not grow**; a combo that grows on focus reflows the row |
| open | as focus | `aria-expanded="true"`, caret rotated |
| invalid | transparent | 1 px `--oc-danger-color` + `0 0 0 3px --oc-danger-ring` — the second published token nothing uses today (`webapp/editor.css:121`) |
| disabled | transparent | none; text `--oc-disabled-color`, `disabled` attribute |

**Menu-bar top-level label (24 tall, radius 6).** Rest transparent
`--oc-text-color`; hover **and** open both `--oc-surface-color` — which is
exactly what `.menu-top:hover, .menu-top[aria-expanded="true"]` already does
(`webapp/editor.css:543`), so this control needs no change. Focus-visible
`2px --oc-accent-color` at `outline-offset: -2px`, inset, because an outset ring
on a 10 px-padded label collides with its neighbour. Pressed is not
distinguished — a menu opens on press, so the open state *is* the feedback. **A
menu-bar label is never disabled**; a menu with nothing in it is hidden by
`applyCommandRules()`'s container count (`webapp/editor.selection.js:1252`).

**Menu / popover row (25 mouse, 44 coarse, radius 6).** Rest transparent; hover
`--oc-surface-color`, **flat, no lift** (`webapp/editor.css:877`, unchanged, and
the deliberate other half of the house split); pressed `--oc-pressed`;
focus-visible `2px --oc-accent-color` inset; selected `--oc-accent-soft` plus a
✓ in `.mi-check` (the pattern `#numfmt-menu button.checked::before` already
draws, `:1497-1500`); destructive rows keep `--oc-danger-color` text (`:891`).
**Disabled uses the `disabled` attribute**, not `.oc-cmd-disabled`'s
`opacity: .45; pointer-events: none` alone (`webapp/editor.css:297`) — an
opacity-dimmed control is still announced as actionable — and the reason goes in
`title`.

**Share and app-bar action pills (28 tall, radius 999)**

| state | container | label + glyph |
| --- | --- | --- |
| rest | `--oc-accent-soft` | `--oc-accent-hover-color` (5.01:1) |
| hover | `--oc-accent-soft-hover` | `--oc-accent-hover-color` |
| pressed | `color-mix(in srgb, var(--oc-accent-color) 34%, transparent)` | `--oc-accent-hover-color` |
| focus-visible | as state | + `2px --oc-accent-color` outline at offset 2, `0 0 0 4px --oc-accent-ring` |
| ungranted (`canShare: false`) | **removed, not disabled** | — |

The last row matters: a capability the host has not granted is not a dimmed
button — `applyCommandRules()` hides it with `.oc-cmd-hidden`
(`webapp/editor.selection.js:1212`), which is what keeps *listed implies
reachable* true for `listCommands()`.

**Panel row / rule list item.** Rest transparent; hover `--oc-surface-color`
**and a 2 px dashed `--oc-accent-color` outline over that rule's range in the
grid**, through the `perQuad` overlay (`webapp/editor.paint.js:891-901`) — that
grid feedback is the whole reason a rule list belongs in a panel; pressed
`--oc-pressed`; selected `--oc-accent-soft` with a 2 px `--oc-accent-color` left
edge; dragging opacity .45, the same treatment `.sheet-tab.dragging` uses
(`webapp/editor.css:1055`).

**Sheet tab.** Unchanged (`webapp/editor.css:1046-1055`): rest
`--oc-muted-text-color`, hover `--oc-text-color`, active
`--oc-background-color` + `--oc-control-shadow` + a colour dot. **This is the
one place a control still lifts and it should stay** — the tab strip sits on
`--oc-surface-color` inside the bottom bar (`:1041-1044`), which is the
*un*-inverted relationship.

**Snackbar and its Undo.** Surface `--oc-tooltip-background-color` / text
`--oc-tooltip-text-color` (`webapp/editor.css:98-99`, `:148-149`) — a dark
surface that already inverts correctly, which is Sheets' `#3C4043` snackbar
expressed in tokens we ship. Undo: rest `--oc-accent-color` on transparent,
hover a 12 % white overlay, pressed 20 %, focus-visible a 2 px
`--oc-accent-ring` inset. **No disabled state** — a snackbar whose action is
dead should not have been shown.

**Presence face and button.** Unchanged (`webapp/editor.css:450-477`). The
typing ring is `box-shadow: 0 0 0 2px --oc-accent-color` (`:477`) and colour is
never the only signal — the roster row says "typing" in words.

### 8.6 Loading, empty, error

**Loading.** Only the version-history and chart panels can be pending. Both show
a skeleton at `--oc-surface-color` **in the item's exact final box**, so nothing
reflows when content lands.

**Empty.** An empty rule list shows one labelled row — *"No conditional formats
on this sheet · Add a rule"* — never an empty panel. An empty `⌄` overflow is
`hidden`, not empty (and it is never empty here, because of the five permanent
residents).

**Error.** A command that throws puts its message **where the pointer is**:
inline in the open menu for a menu refusal, under the offending field for a
panel refusal, in the snackbar for a grid refusal. The control returns to rest
and never latches. `#tb-status` keeps only what it is for — engine and load
state — which narrows today's behaviour, where `statusError()`
(`webapp/editor.i18n.js:200-206`, bound at `webapp/editor.core.js:10412`) sends
every message to a 34 px strip ~700 px from the click, in an element with no
`role` and no `aria-live`.

**And enablement instead of refusal.** A command that cannot run is `disabled`
in the menu with the reason in `title`, evaluated by the predicate slot `MENUS`
already supports (the fourth tuple element, used for ticks at
`webapp/editor.core.js:9831-9832`). The sharpest case is
`pasteSpecialDialog()`, which returns early with
`status.textContent = "clipboard is empty"` (`webapp/editor.dialogs.js:2091`) —
an enabled menu item that refuses silently, which is what `docs/82:28` recorded
as "Paste special / did not open".

---

## 9. Motion

The owner asked for fluid UX. A design note that leaves motion to the
implementer has not specified the product.

### 9.1 The token set

`docs/91` §2.4 already names a vocabulary; two chromes must not ship two.
**Adopt it verbatim, extend by three, re-point nothing.**

```css
/* durations */
--oc-dur-0:  0ms      /* press-in; anything on the grid's critical path */
--oc-dur-1:  100ms    /* hover in, press out */              (docs/91; = .tb-btn's shipped .1s, editor.css:765)
--oc-dur-2:  140ms    /* caret rotation, small crossfade, menu exit */ (docs/91; beside :429's 120ms)
--oc-dur-3:  160ms    /* menu / popover / picker open */     (docs/91)
--oc-dur-4:  220ms    /* snackbar enter, avatar join, compact controls */ (docs/91)
--oc-dur-5:  280ms    /* reserved — backstage, ribbon body */ (docs/91, unused here)
--oc-dur-6:  300ms    /* NEW — side panel + grid. M3 `medium2` */

/* easing */
--oc-ease-standard: cubic-bezier(0.4,  0,    0.2,  1)     (docs/91; = M3 `legacy`)
--oc-ease-out:      cubic-bezier(0.16, 1,    0.3,  1)     (docs/91)
--oc-ease-in:       cubic-bezier(0.7,  0,    0.84, 0)     (docs/91)
--oc-ease-emphasis: cubic-bezier(0.2,  0,    0,    1)     (docs/91; = M3 `emphasized`)
--oc-ease-decel:    cubic-bezier(0.05, 0.7,  0.1,  1)     /* NEW — M3 emphasized-decelerate */
--oc-ease-accel:    cubic-bezier(0.3,  0,    0.8,  0.15)  /* NEW — M3 emphasized-accelerate */
```

The three M3 values are from the AndroidX Compose `MotionTokens.kt` source
(accessed 2026-09-09; URL in §2), the generated canonical form of the m3
token table.

**The rule that goes with them, and it governs every entry below:** things
*entering* use a decelerate curve and the longer duration; things *exiting* use
an accelerate curve and roughly half the time.

Motion today is **five `transition:` declarations and one `@keyframes` in 2,299
lines** (`webapp/editor.css:429`, `:765`, `:967`, `:1394`, and the presence
pulse at `:526`), so everything here is additive.

### 9.2 The eleven transitions

**1 — Menu open and close.** Open: opacity 0→1 over `--oc-dur-2`
`--oc-ease-decel`, plus `scaleY(0.94)→1` over `--oc-dur-3` `--oc-ease-decel`,
`transform-origin` at the corner nearest the trigger — `anchorMenu()` already
computes which side has room (`webapp/editor.core.js:8806`). The
`--oc-popover-shadow` is **static on the animated layer**, never transitioned;
shadow is paint-bound. Close: opacity → 0 over `--oc-dur-1` `--oc-ease-accel`,
**no scale** — a panel that shrinks as it leaves reads slower than one that
simply goes.

> **Switching between adjacent menus while the bar is armed is 0 ms, no
> animation at all.** This is the single detail that separates a native-feeling
> menu bar from a web one, and it is a *restraint*, not new code: `openMenu(i)`
> already swaps drops synchronously from the arrow and Alt paths
> (`webapp/editor.core.js:10201-10234`). The contract is that the open
> transition is suppressed when a menu is already open.

**2 — Side panel sliding in, and the grid meeting it.** The important one.

- **Open.** The panel goes `position: absolute; right: 0; top: 0; bottom: 0`
  with `will-change: transform`, and translates `translateX(100%) → 0` over
  `--oc-dur-6` `--oc-ease-decel`. **The grid does not resize during the
  animation.** On `transitionend` the panel returns to flow
  (`position: static; flex: 0 0 W`) and `resize()` runs **exactly once**.
- **Why there is no visible jump.** The panel is opaque and docks flush right,
  so the columns it covers during the slide are exactly the columns the reflow
  clips. Overlay-then-reflow is **pixel-identical** to a continuously reflowing
  panel, at 1 `resize()` instead of ~18. `resize()` reads
  `wrap.getBoundingClientRect()`, reallocates the canvas **and calls `draw()`**
  (`webapp/editor.geometry.js:229-238`); running that per frame is direct
  contention with 60 fps over a million cells.
- **The one thing that does move.** `.vscroll` is a child of `.grid-wrap` pinned
  to its right edge and would be underneath the panel during the slide, then
  jump. It animates `transform: translateX(−W)` on the same duration and curve,
  reset on `transitionend`. Compositor-only.
- **Close.** Mirror image: the panel goes absolute *first*, `resize()` runs
  **once immediately** so the grid widens while it is still covered, then the
  panel translates out over `--oc-dur-4` `--oc-ease-accel`.
- **The invariant, in one sentence:** *the grid reaches its final size at the
  moment it is covered, never while it is exposed* — so `resize()` runs last on
  open and first on close, exactly once either way.
- Panel content fades in with a `--oc-dur-1` delay over `--oc-dur-2`, so the
  panel arrives before its contents.

Below 1024 px the panel is an absolutely-positioned overlay for good and
`resize()` never runs at all (§10).

**3 — Snackbar rising.** Enter `translateY(16px) → 0` + opacity 0→1 over
`--oc-dur-4` `--oc-ease-decel`. Dwell **6 s**. Exit opacity → 0 +
`translateY(0 → 8px)` over `--oc-dur-2` `--oc-ease-accel`. Never more than one;
a second replaces the first with a 100 ms crossfade rather than stacking. It
does not take focus and does not block input. The 6 s and the dismiss-on-edit
clause are correctness, not taste (§7.3).

**4 — "Saving… → Saved". The signature transition, and the smallest.** Entirely
in `#doc-state` (`webapp/editor.html:77`).

- "Saving…" appears with **`--oc-dur-0` — no animation at all.** It must not
  draw the eye.
- On settle: crossfade to "All changes saved" over `--oc-dur-2`
  `--oc-ease-standard`, opacity only, on two absolutely-stacked spans so nothing
  tweens width.
- After a **2,000 ms** hold the label collapses to the check glyph: `max-width`
  → 0 with `overflow: hidden` over `--oc-dur-3` `--oc-ease-standard`.
  `max-width` is layout-affecting, but it is one element in the **app bar's**
  flex row: the band's height never changes, so `resize()` is never called and
  the grid never learns this happened.
- Failure: "Not saved — ⟨reason⟩" appears with no animation, in
  `--oc-danger-color`, and **does not collapse**.
- **Nothing here is a toast, a spinner or a modal.**
  [83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) §5.5's "a standing indicator,
  not a toast" is already the house position.

**5 — Presence avatar joining and leaving.** Join `scale(0.6) → 1` + opacity
0→1 over `--oc-dur-4` `--oc-ease-decel`. Leave `scale(1) → 0.6` + opacity → 0
over `--oc-dur-2` `--oc-ease-accel`, then removal. **The row's 13 px of width
change is not tweened** — 13 px of travel is invisible and a width tween on a
flex row is a layout animation per frame in the same band as the document title.
The existing typing pulse (`@keyframes oc-presence-pulse`, 1.4 s,
`webapp/editor.css:526`) is kept verbatim, including its targeted reduced-motion
guard at `:527`.

**6 — A remote collaborator's cursor moving.** This is **grid content, not
chrome** — drawn on the canvas by `webapp/editor.paint.js:891-925`. So it is a
JS tween, and it gets the strictest budget in this document:

- Interpolate `translate` over `--oc-dur-4` `--oc-ease-standard` **only** when
  the move is inside the current viewport and within ~12 columns / 24 rows.
  Otherwise **cut instantly** — a rectangle flying across 40 columns reads as
  noise, not as presence.
- The tween samples on the existing `draw()` schedule. If a frame is already
  pending it costs nothing; if not it requests at most one
  `requestAnimationFrame` per 16 ms and **stops the instant the tween settles**.
- **Hard cap: at most one tween in flight per participant, and if more than
  eight participants are moving at once every cursor cuts.** That cap is the
  frame-budget guard and it belongs in the spec, not in a comment.
- The name flag fades out after 3 s of stillness over `--oc-dur-2` and reappears
  **instantly** on the next move — a flag that fades in is unreadable in the
  moment you look for it.

**7 — Toolbar overflow opening.** Exactly like a menu — `--oc-dur-3`,
`--oc-ease-decel`, scale from the button's top corner. **The reflow that moves
controls into and out of chips is `--oc-dur-0`, never animated.**
`reflowToolbar()` moves real nodes (`webapp/editor.core.js:9361`+), and
animating a resize-driven reflow produces jitter for the whole duration of a
window drag.

**8 — Selection and the fill handle. Both 0 ms, always.** The active-cell
border and the range wash are canvas paint; an eased selection is an extra frame
per arrow keypress on the grid's critical path, and a spreadsheet selection that
eases feels laggy rather than smooth. The marching-ants marquee keeps its
existing JS crawl, which is the **only** JS animation in the product; the Sheets
chrome adds none.

**9 — Focus rings: not animated, in any state, ever.** A ring that fades in is a
ring that is absent for the first frame after a keypress, and keyboard users
arrive at speed.

> **This needs one concrete fix, and both chromes inherit the defect.**
> `.tb-btn` ships `transition: background .1s, color .1s, box-shadow .1s`
> (`webapp/editor.css:765`), and both this design and `docs/91` §2.3 specify the
> focus ring as `outline` **plus** `0 0 0 4px var(--oc-accent-ring)` — a
> `box-shadow`. As written, the ring fades. One line:
> `.tb-btn:focus-visible { transition: none; }`, in whichever chrome ships
> first.

**10 — Compact controls (Ctrl+F1).** 64 px of chrome height, which *is* a grid
resize, so it takes the panel's contract inverted: the bands go
`position: absolute` over the grid; on collapse the grid grows first (still
covered) and the bands then `translateY(−64px)` over `--oc-dur-4`
`--oc-ease-emphasis`; on restore the bands translate down first (covering) and
the grid then shrinks. One `resize()` each way, same one-sentence invariant.

**11 — Filter-view mode entry.** The dark banner slides down from above the
column headers (`translateY(−100%) → 0`, `--oc-dur-6` `--oc-ease-decel`) while
the header bands cross-fade from light to dark on the same duration. Because
this is a **mode** change rather than a panel it earns the longest and most
emphatic transition in the chrome — which is Material 3's own rule that bigger
state changes get `emphasized` and more time.

**Toggle, hover and press.** Hover `--oc-dur-1`. Press **0 ms in, `--oc-dur-1`
out** — a press must be instantaneous and the release elastic; a 100 ms
press-in delivers feedback after a fast clicker's finger has left. A toggle
latching to `--oc-accent-soft` crossfades over `--oc-dur-1`. **Nothing scales on
press**: 0.97 on a 28 px control is 0.8 px of travel, invisible, and it costs a
compositor layer per control.

### 9.3 Deliberately not animated, and why

- **Selection, the fill handle, gridlines, freeze lines** — every canvas paint
  except the capped presence tween.
- **Toolbar reflow membership.** Jitter during a drag.
- **Focus rings.** Absent for the first frame is worse than not animated.
- **`#tb-status` text.** 84 of the 88 browser specs boot on it reading
  `/^engine v\d/`; a faded or delayed status line is a suite-wide flake
  generator.
- **The app bar's own height.** Only compact controls changes it, and that is a
  deliberate mode.
- **No `backdrop-filter` anywhere.** It forces a full-surface readback per
  frame, and `docs/88` §1.2 already deleted the one instance we had.
- **No simultaneous `box-shadow` transitions.** Shadow is paint-bound.
- **No JS timers in the chrome.** The presence tween is grid content and is
  capped; the marching ants predate this; nothing else.
- **Nothing in the chrome transitions while a grid frame is pending.** A root
  class `.oc-grid-busy` applies `transition: none` across the header block, set
  when `draw()` is scheduled and cleared once it lands. **This is the mechanism
  that makes every duration above safe rather than merely small.**
- **Scroll does not collapse the chrome.** Tying a height change to a scroll
  event is the worst possible coupling to the grid's critical path.

### 9.4 The `prefers-reduced-motion` contract

The global clamp at `webapp/editor.css:1505` (`docs/91` §2.4 cites `:1503`;
verified, it is `:1505`) already forces `transition-duration` and
`animation-duration` to 0.01 ms on everything, so every CSS transition above
inherits the guard for free.

**Become instant, by the clamp, no extra work:** menu open and close, popover
and picker open, the snackbar's rise and fall, the panel slide and the vscroll
shift, compact controls, avatar join and leave, the save-state crossfade and
collapse, caret rotations, hover and press colour swaps.

**Need a targeted `transition: none` beyond the clamp** — a 0.01 ms transform is
still a transform and can land visibly mid-frame — following the pattern already
at `webapp/editor.css:431` and `:527`: the panel's `translateX`, the vscroll's
`translateX`, the compact-controls `translateY`, the avatar's `scale`, and the
filter-view banner's `translateY`.

**Need a JS check, because the clamp only reaches CSS:** the presence-cursor
tween. Check `matchMedia("(prefers-reduced-motion: reduce)")` **per move, not
once at boot** — the same per-event discipline `webapp/editor.core.js:10046`
already uses for hover capability — and cut.

**Merely shortened: nothing.** Two timings are deliberately *outside* the
contract because they are dwell, not motion: the snackbar's 6 s (an Undo you
cannot reach is not an Undo) and the save state's 2 s hold. Both are
`setTimeout`s, so the clamp does not touch them; the point is that this is a
decision rather than an oversight.

**The hard rule, adopted verbatim from `docs/91` §2.4: the reduced-motion end
state must be pixel-identical to the animated one.** A reduced-motion path that
also changes layout is a second design nobody reviewed. The gate compares
settled bounding boxes rather than asserting that durations are zero.

---

## 10. The responsive contract

### 10.1 By viewport

§6.4 is the toolbar's ladder. This is everything else.

| viewport | app bar | menu bar | panel |
| --- | --- | --- | --- |
| **2560** | full | full | in flow, 400 (the `clamp` ceiling) |
| **1920** | full | full | in flow, 400 (24vw = 461 → clamped) |
| **1440** | full | full | in flow, 346 |
| **1366** | full | full | in flow, 328 |
| **1280** | save state starts **collapsed to its icon**, expanding only on a state change | full | in flow, 307 |
| **1024** | presence stack caps at **2 + "+N"** instead of 3 | full | **becomes an overlay** — §10.2 |
| **900** | the `⋮` absorbs `#tb-settings` | full | overlay, `min(400px, 100vw − 56px)` |
| **768** | Share loses its label → icon-only pill, `aria-label` retained | full | overlay |
| **≤ 560** | document name, save-state icon, Share icon, avatar stack — nothing else | the eight labels fold to `☰` + the tool finder, through the `reflowMenubar()` / `#menu-more` machinery that already exists (`webapp/editor.html:168-170`, `webapp/editor.core.js:10160`+) | full-height sheet |

### 10.2 The panel, and the floor

- **≥ 1024** — an in-flow sibling at `clamp(280px, 24vw, 400px)`. The grid
  reflows to meet it, once, per §9 item 2.
- **640 – 1024** — absolutely positioned inside `.work-area`
  (`webapp/editor.css:1297`) with a scrim, at `min(400px, 100vw − 56px)`. **The
  grid keeps its full width beneath, so `resize()` does not run at all in this
  range.**
- **< 640** — a full-height sheet on the same absolute path.

**The floor: the grid is never below 480 px of usable width.** Today
`.side-panel { flex: 0 0 316px }` (`webapp/editor.css:1301`) against
`.grid-wrap { flex: 1 1 auto; min-width: 0 }` (`:1298`) with no `@media` rule
anywhere touching `.side-panel`, so at 800 px the grid keeps 484, at 560 it
keeps 244, and at 390 the document is a **74 px sliver behind the tool
configuring it**.

The arithmetic floor for the in-flow path is **760 px**
(`W − clamp(280,…) ≥ 480`), not 1024. Switching at 1024 therefore carries 264 px
of deliberate headroom — an authored number with margin rather than a measured
edge, which is the whole lesson of the 1461.

### 10.3 Touch

Under `(pointer: coarse)` the toolbar band goes to **48** and the formula bar to
**40**, exactly as they do today (`webapp/editor.css:2204`-region), with
`.tb-btn` at 44 × 44 inside a 40 px container, and menu rows at 44
(`:2181`+). `tests/browser/editor.touch-targets.spec.mjs:145` caps total
coarse-pointer band growth across `.toolbar` + `.formula-bar` at **16 px**; the
Sheets chrome grows the toolbar 40 → 48 and the formula bar 35 → 40, which is
**13 px** and inside the cap.

The app bar and menu bar do **not** grow — the menu bar's labels are text
targets, and the app bar's tallest control is already 32 and goes to 44 through
the existing `.tb-icon` coarse rule, which fits a 44 + 2 + 2 = 48 band. **State
that as a 12 px coarse growth on the app bar, outside the two-band cap, and
re-take the assertion parameterised by chrome** — §14 R-7.

### 10.4 What never collapses, at any width, in any state

- **The document name.** Ellipsised (`max-width: min(52vw, 460px)`,
  `webapp/editor.css:325`; `42ch` on the rename input at `:2260`-region), never
  removed — it is the only thing that says which file you are in.
- **The save state**, in at least its icon form.
- **The Share affordance**, in at least its icon form. Hiding the entry point to
  the collaborative product until collaboration is already happening is a
  circular dependency.
- **The presence stack**, whenever a session exists.
- **The menu bar's existence** — `☰` at worst. `menuModel()` filters on
  `.oc-cmd-hidden`, not visibility (`webapp/editor.selection.js:1128-1131`), and
  `desktop/src/main.rs:927` polls `menuModel().length`: **an empty model does
  not fail, it hangs the shell forever.**
- **The formula bar's Name Box and input.**
- **`#tb-status`** in the status bar
  (`tests/browser/editor.chrome-composition.spec.mjs:112`).
- **The sheet-tab strip**, with `＋` and `☰` pinned left, outside the scroller
  (`webapp/editor.sheets.js:283`).
- **Every command's DOM node.** A collapsed group's controls **move** into its
  chip panel; they are never destroyed and never cloned — the contract
  `collapseGroup`/`expandGroup` already keep (`webapp/editor.core.js:9361`+) —
  because `listCommands()` reads the live DOM
  (`webapp/editor.selection.js:1040-1046`) and a lazily-rendered surface
  silently shrinks the SDK's published surface as the user resizes the window.
- **`ChromeRegions` gains no new name.** `CHROME = ["header","menubar",
  "toolbar","formulabar","tabs","statusbar","localePicker"]`
  (`webapp/embed.js:97-99`) throws on unknown names at `:264`, so old hosts
  cannot feature-detect a new one and adding one is a major version.
  `toolbar: false` must keep hiding whatever occupies the toolbar's role, so the
  Sheets container joins the existing `.oc-hide-toolbar .toolbar` rule
  (`webapp/editor.css:253`).

---

## 11. Keyboard, and ARIA

### 11.1 The shortcut map

Every chord below was checked against the live handler before being claimed.
**Nothing existing is rebound** — the file's own comment at
`webapp/editor.core.js:8461-8469` argues that a chord doing something *else* is
worse than one that is missing, and it has been paid for twice already.

| chord | verb | status |
| --- | --- | --- |
| **Alt+/** | Tool finder ("Search the menus") | **free.** The Alt handler (`webapp/editor.core.js:10168`+) looks up `menuMnemonics.get("/")`, undefined on a letter-keyed map, and returns. Sheets' own binding |
| **Ctrl+/** | Keyboard-shortcut sheet | **free.** `grep 'key === "/"' webapp/*.js` returns nothing. Sheets' own binding |
| **Ctrl+F1** | Compact controls | **free.** The F-key switch (`webapp/editor.core.js:8632-8654`) handles F2, F5, F9 and F11 only |
| **F6 / Shift+F6** | Landmark cycling | **free.** `grep -rn '"F6"' webapp/` returns nothing |
| **Ctrl+Alt+M** | Insert comment | **free.** Sheets' own |
| **Ctrl+Alt+Shift+A** | Open the comment thread list | **free.** Sheets' own |
| **Alt+Shift+5** | Strikethrough | **free**, offered as a Sheets-migrant alias |
| **Alt+F/E/V/I/O/D/T/H** | open the matching menu | **already ship** — derived per locale at relabel time (`webapp/editor.i18n.js:87-108`) |

> **Ctrl+Shift+F is Sheets' compact-controls chord and it is not available
> here.** It is Format Cells (`webapp/editor.core.js:8470`), moved off Find
> deliberately, with nine lines of comment at `:8461-8469` arguing exactly why
> not to do this again. So compact controls takes **Ctrl+F1** — which is also
> the chord `docs/91` §7.2 claims for the ribbon's collapse, so **one verb
> ("give the chrome's height back to the grid") has one chord across both
> chromes.** That is a better outcome than either chrome matching its source.
> Record the divergence in the release note; do not fix it by moving Format
> Cells.

**No new key space, and this is the Sheets chrome's largest keyboard advantage
over the ribbon.** `docs/91` §7.1 has to build a per-locale `keytip.<id>`
catalogue, a uniqueness validator at catalogue load, and an audit of
`desktop/src/menu.rs`'s `releases_during_edit()`, because Excel publishes fixed
tab letters that must not be derived. Sheets has no KeyTips — its menu bar *is*
the access surface — so the existing first-free-letter-of-the-translated-label
mechanism is **correct here as it stands**, and none of that work is incurred.
`tests/browser/editor.excel-shortcuts.spec.mjs` stays green untouched.

### 11.2 Menu access

The existing menu bar *is* the Sheets menu bar, so it is **visible** rather than
hidden and `openMenu()` never measures a `display:none` button's
`getBoundingClientRect()` (`webapp/editor.core.js:8806`) — the clickable-ghost
hazard `docs/91` §6.2 has to engineer around does not arise here at all.

**Except in one state, and it must be handled explicitly.** While compact
controls is on, the menu bar *is* hidden, so the ghost hazard arrives by another
door: `openMenu(i)` must refuse while compact, and **Alt+letter must un-compact
first and then open, on the next frame**, so `anchorMenu()` measures a shown
button. That is the one new rule the compact mode costs.

Within a menu, everything already works and is kept verbatim
(`webapp/editor.core.js:10198-10234`): Up/Down through rows, Left/Right to the
neighbouring menu, Home/End to the ends, Escape back to the bar with focus
restored. Submenus open on hover only where hovering exists —
`matchMedia("(hover: hover) and (pointer: fine)")` checked **per event, not
once at build time**, because Chrome replays `mouseenter` before `click` at a
touch point (`webapp/editor.core.js:10046`).

### 11.3 F6 landmark order — entirely new work

Forward, `Shift+F6` reversing:

1. **Menu bar** — the tool finder, then File
2. **Toolbar** — one composite stop
3. **Formula bar** — the Name Box
4. **Grid** — `#grid` (`webapp/editor.html:558`)
5. **Side panel** — only when open
6. **Sheet-tab strip** — `#sheet-tabs` (`webapp/editor.html:587`)
7. **Status bar**
8. **App bar** — doc name → save state → presence → Share → settings → wraps to 1

It starts at the menu bar rather than the app bar because F6 exists to get *out
of the grid and to the commands*; the app bar is last because its contents are
about the document, not about the data. Each landmark takes focus on the element
**last focused inside it**, falling back to its first control. The side panel is
in the cycle only when open — a landmark that is present but empty is a dead
press.

Eight landmarks to the ribbon's seven, because the app bar and the menu bar are
separate regions here where the ribbon merges its tab strip and body. **Both
orders put the grid third and the panel fourth**, which is the part that matters
for muscle memory across the two chromes.

### 11.4 Arrow roving

**Menu bar.** Unchanged: `focusTop()` keeps exactly one `tabIndex = 0`
(`webapp/editor.core.js:10186-10192`).

**Toolbar.** One tab stop for the whole row, unchanged (`:9438-9476`).
Left/Right cross group boundaries because `items()` is a flat list (`:9440-9441`).
Home/End to the ends. `Down` on a split or menu button **opens its menu**. Text
fields keep their own Left/Right — the carve-out at `:9464`.

**`syncStops()` must sweep controls inside a closed panel and a closed chip
flyout too.** It already resets `tabIndex = -1` on everything in the toolbar
including nodes parked in a collapsed group's flyout (`:9453`), precisely
because they would otherwise become extra tab stops the moment the flyout
opened. A rule-list panel with twenty controls reproduces that trap at scale.

**A click inside a flyout must not dismiss it** (`:9430`). Any Sheets-chrome
popover holding a live input — the font box, a hex field, the tool-finder search
— needs the same carve-out.

### 11.5 Focus into and out of a panel

- **Open**: focus to the panel's **first control**; `#side-panel-title`
  (`webapp/editor.html:577`) announced through `#grid-live` (`:560`).
- **Not a focus trap** above 1024 px. `role="complementary"`, non-modal; Tab
  from its last control leaves into document order, which is correct — the grid
  is what the panel is about and must stay clickable while a rule is written.
  Below 1024, where the panel is an overlay with a scrim, it becomes a real
  trap; §11.6.
- **Escape is scoped, and today it is not.**
  `webapp/editor.core.js:10245-10248` closes the panel on any `Escape` while
  `activePanel` is set, with **no check on `e.target`** — so Escape inside the
  conditional-formatting value fields (`webapp/editor.dialogs.js:1547-1548`),
  the custom-formula field (`:1561`) or the comment textarea
  (`webapp/editor.core.js:5541-5543`) destroys everything typed, and `openPanel`
  clears the body on the next open (`:5160-5161`), so the draft is
  unrecoverable. The new contract is one delegated handler on `#side-panel`:
  inside a text-entry control, Escape reverts that control and blurs; on the
  panel chrome, or on an already-clean field, Escape closes the panel.
  **The mechanism is worth recording:** the guard was applied correctly at two
  sites — the table rename field (`webapp/editor.dialogs.js:1090`) and the chart
  text fields (`webapp/editor.core.js:4449`) — and never generalised, which is
  why the defect is invisible to anyone who tests the two panels that happen to
  work.
- **Close**: focus returns to the **invoker**, not unconditionally to the
  canvas, which is what `closePanel()` does today
  (`webapp/editor.core.js:5463`+).

### 11.6 ARIA

**App bar.** `<header class="app-bar" role="group" aria-label="Document">`.
**Explicitly not `role="banner"`** — an embedded editor is not its host page's
banner, and inside `<opencalc-sheet>` a second banner is a landmark collision
the host cannot see coming. Landmark movement here is F6, not ARIA landmarks,
because a shadow-DOM editor's landmarks do not aggregate reliably with the
host's.

`#doc-name` keeps `aria-label="Rename the document"` (`webapp/editor.html:72`).
`#doc-state` (`:77`) **gains `role="status" aria-live="polite"`**, which it does
not have today — which is why the one indicator that says whether the user's
work is safe is announced to nobody. When the label collapses to its check
glyph the text **stays in the DOM as `.sr-only`** rather than being replaced by
an `aria-label`: a name that lives in one place cannot drift from the text it
replaced.

**Menu bar.** `role="menubar" aria-label="Menus"` — **not "Sheets"**, because
`#sheet-tabs` already owns `role="tablist" aria-label="Sheets"`
(`webapp/editor.html:587`) and two similarly-named composite widgets leave a
screen-reader user unable to tell which they landed in. Top-level items
`role="menuitem" aria-haspopup="menu" aria-expanded`; rows `menuitem` /
`menuitemcheckbox` / `menuitemradio`; the popup `role="menu"`.

**Menu row markup is a three-slot contract and does not change**: `.mi-check` /
`.mi-label` / `.mi-key`. `menuModel()` reads the label from `.mi-label`,
`refreshChecks()` writes the tick into `.mi-check`, and `relabel()` writes
translations into `.mi-label` (`webapp/editor.core.js:10058-10060`,
`webapp/editor.i18n.js:118-127`). Change the row markup and the OS menu loses
labels, accelerators and ticks at once. Labels are set with `textContent`, never
`innerHTML` — a translated label is host-supplied text.

**Toolbar.** `role="toolbar" aria-label="Formatting" aria-orientation="horizontal"`,
one composite tab stop. **The rounded container is presentational** — no role,
no label; it is a surface, and announcing it would insert a meaningless group
between the toolbar and its controls. Groups are `role="group"
aria-label="⟨caption⟩"`; the caption is never drawn (Sheets draws no group
captions) but the label stays, because the grouping is still true and only
visually unlabelled. Separators gain `role="separator"
aria-orientation="vertical"`.

**Split buttons** are a wrapper `role="group"` containing **two real
`<button>`s**: primary `aria-label="Fill colour"`, caret
`aria-label="Fill colour options" aria-haspopup="menu" aria-expanded`. Two names
because they are two things; the caret carries `data-oc-proxy`, never a second
command id.

**Toggles** are `<button aria-pressed>`, not `role="switch"`.
`aria-pressed="mixed"` is already handled visually (`webapp/editor.css:1487-1493`)
and must stay.

**Combo boxes** are `role="combobox" aria-expanded aria-controls` on the input
with `role="listbox"`/`option` on the popup and `aria-activedescendant` while
navigating — what `wireCombo` already builds (`webapp/editor.core.js:9114`).
**`123 ▾` must be built the same way**, not as a menu button with a text span:
today `#tb-numfmt-label` is a `<span>` (`webapp/editor.html:402`) stamped as a
command by the boot sweep, so `runCommand("toolbar.numfmt-label")` would
`.click()` a span.

**Side panel.** `<aside role="complementary" aria-labelledby="side-panel-title"
tabindex="-1">`. Today it carries a static `aria-label="Tool panel"`
(`webapp/editor.html:575`); replace that with `aria-labelledby` pointing at
`#side-panel-title` (`:577`) so the accessible name is the *tool's* name —
"Conditional formatting" — rather than the word "panel". **Below 1024 px it
becomes `aria-modal="true"` with a real focus trap**, driven by the same
`matchMedia` that drives the layout: a scrim that is not modal is a lie to both
the eye and the screen reader. Rule lists are `role="list"` / `listitem`; a
reorderable list adds `aria-roledescription` and keyboard reorder
(Alt+Up/Down), because drag alone is not an interaction.

**Snackbar.** `<div role="status" aria-live="polite" aria-atomic="true">` with a
real `<button>` for Undo. **Not `role="alert"`** — assertive interrupts, and
this reports something the user just did on purpose. It does not take focus and
is not a keyboard trap because it is not a keyboard target; the keyboard route
is the chord that already exists.

**Presence.** Unchanged and correct as authored: `#presence-btn` carries
`aria-haspopup="true" aria-expanded aria-controls="presence-menu"
aria-label="Collaborators"` (`webapp/editor.html:182-185`); the overlapping
faces are `aria-hidden="true"` (`:186`); `#presence-menu` is `role="menu"
aria-label="Collaborators"` (`:188`). The only change is that the label carries
the count. **Join and leave are announced through `#grid-live`**, never by
putting `aria-live` on the roster.

**Status bar.** `#tb-status` **gains `role="status"`** — it has none today,
unlike `#grid-live` (`:560`) and `#autosave-state` (`:616`).

**Cross-cutting.** Disabled is the `disabled` attribute, not
`.oc-cmd-disabled`'s opacity alone (`webapp/editor.css:297`). Every inactive
surface needs its own `[hidden] { display: none }` rule — the trap has been
sprung four times (`webapp/editor.css:535-541`). Every icon-only control gets an
explicit `aria-label`, because `initTooltips()` promotes `title` → `aria-label`
for four selectors only (`webapp/editor.core.js:5730`-region) and a control
outside them loses its accessible name silently. All surfaces stay in the DOM;
unavailability is `disabled`, never absence. **No pure-chrome element may carry
an `id` starting `tb-`.** The overlay host is `ocOverlayHost`, never
`document.body` (`webapp/editor.core.js:1432-1437`).

**Contrast.** The focus ring clears 3:1 against both the control's own fill and
the container behind it. The container-to-ground tonal step is 1.03:1 and is
never the only signal. **No state is signalled by colour alone**: a checked
toggle also swaps to its filled icon, a coloured sheet tab also draws a 6 × 6
dot (`webapp/editor.css:1050-1053`), a typing collaborator is also named in
words.

---

## 12. The dual-chrome architecture, and the chrome chooser

The product owner's requirement, verbatim: **"we need both UIs, which can be
configured based on the user's preference"**, and **"specially the Excel UI for
the desktop version"**. This section specifies both halves — the seam that makes
two chromes affordable, and the chooser.

### 12.1 The question that decides everything else: can a chrome own a command id?

**Today it can, and that is the thing to change first.** The boot sweep stamps
`data-oc-command = "toolbar." + id.slice(3)` onto every `[id^='tb-']` node
(`webapp/editor.core.js:11019-11021`), so ~30 published ids are owned by the
toolbar's own markup — and `docs/91` §3.3 assigns those same ids to ribbon
controls. DOM ids are unique; **two chromes cannot both own them.** Worse,
`runCommand` dispatches to `qsa(...)[0]`, the first node in document order
(`webapp/editor.selection.js:1146-1147`), so with both chromes present the
*invisible* one silently receives every programmatic click — including the ones
that then anchor a popover to a zero-box node (`:8806`).

**Therefore neither chrome may own an id.** The owners move into one hidden,
chrome-neutral **registry**. The menu tree already *is* exactly that for the
`file.*`/`edit.*`/`view.*` ids — `ADR-026` clause (3), proven by the desktop
shipping `#menubar { display: none }` with a complete OS menu
(`webapp/editor.css:2055`) — and the `toolbar.*` owners join it. Both chromes
become **100 % proxies** carrying `data-oc-proxy="<id>"`.

**Consequence: `listCommands()` is identical under both chromes by
construction, not by a test.** And four of the risks a dual chrome would
otherwise carry are answered *structurally*: the capability and read-only regex
tables need no per-chrome branch because no id is renamed; `command.<id>` and
`tip.<id>` translate both chromes for free because keys are ids; and the desktop
OS menu is right under both because neither chrome feeds it.

**One thing this makes visible that a single chrome never would.**
`applyCommandRules()` sweeps `[data-oc-command]` only
(`webapp/editor.selection.js:1212`). A proxy carries `data-oc-proxy`, so **a
proxy for a command a host hid stays visible and clickable, and a pointer click
on it clicks the hidden owner.** `runCommand` refuses that path (`:1157-1160`);
a mouse does not go through `runCommand`. That is security-adjacent, it is
created by this design, and it is red the moment the first proxy ships. The
cascade gains a `[data-oc-proxy]` pass mirroring the owner's hidden/disabled
state, and its group-emptiness sweep (`:1252-1276`, hard-coded to `.tb-group`
and `.toolbar`) becomes manifest-supplied — or a read-only viewer gets a live
ribbon band above an empty grid.

### 12.2 The shared layer, and its exact seam

**Shared — one of each, no per-chrome branch:**

| | why it cannot be duplicated |
| --- | --- |
| The command registry and its ids | §12.1 |
| `CAPABILITY_COMMANDS` (`webapp/editor.core.js:1336-1358`) and `READ_ONLY_SAFE` (`:4993-5008`) | both match by regex; a second table is a second answer to "may this viewer save" |
| `applyCommandRules()` (`webapp/editor.selection.js:1166-1286`) | one governance cascade, extended per §12.1 |
| `listCommands()` / `menuModel()` / `runCommand()` (`:1040`, `:1065`, `:1146`) | published SDK surface; `desktop/src/main.rs:927` hangs forever on an empty model |
| i18n: `command.<id>`, `tip.<id>`, and **one** new `chrome.<slug>` space | keyed by id, so proxies resolve for free. Never `ribbon.*` + `sheets.*` — a host ships one catalogue, and two spaces guarantee a permanently half-translated product |
| The icon set — one licensed sprite, one stroke weight, one third-party notice | two sets is two licences, two notices, two weights, and a product whose two skins visibly are not one product |
| The grid (`webapp/editor.paint.js`, `webapp/editor.geometry.js`) | a chrome supplies a declared band height and nothing else; the entire coupling is one `resize()` per change |
| The formula-bar core — one `#formula-input`, one mirror, one Name Box | restyle only: one DOM, two stylesheets |
| The dialog and panel layer — `#oc-modal`, `openPanel()`, `#side-panel`, `anchorMenu`, `ocOverlayHost` | **this is where §7's wins land, and they land for both chromes.** Excel ships task panes too |
| One keyboard dispatcher and one accelerator table | Ctrl+B is Ctrl+B in both. Only the *discovery* layer diverges |
| One theme token set, all three blocks | a token declared by one chrome only breaks the OS-dark path for that chrome |
| Status bar, `#tb-status`, `#grid-live`, sheet-tab strip, presence roster | below the chrome bands, belonging to neither. **The roster relocation is done once, to a region both chromes have** — and both have an app/title strip |

**Built twice, and this is the whole price:**

- Two band markups and **two stylesheets with two complete state vocabularies**
  (~200-400 lines each).
- Two layout assignments — **and they are not the same cost.** The ribbon's is
  ~190 commands × (tab, group, position, size, collapse order). This chrome's is
  the inverse and much smaller: choose 30 toolbar seats by §6.1's threshold,
  plus a menu-path assignment that is nearly free because seven of the eight
  menu names already match. Do not price them the same.
- Two keyboard-discovery layers — KeyTips vs the tool finder. Neither ports.
- Two File surfaces — the Backstage vs a 17-item File menu routing to panels.
  **Three rules underneath the Backstage are chrome-agnostic and expensive to
  rediscover** (`docs/91` §6.1): it is a *route* filtered against a known list
  before anything reaches a class; `resize()` runs on exit and **only** on exit,
  because `.work-area` is `display: none` on the way in and measures 0 × 0; and
  an open cell editor is **committed, not abandoned**. Extract those three.
- Two collapse contracts that **must not be unified into one setting**: the
  ribbon's Ctrl+F1 (collapse the body, keep the tabs) and this chrome's compact
  controls (fold the app bar and menu bar, keep the toolbar) collapse different
  regions in opposite directions.
- Two contextual renderings — ribbon tabs vs panels — over the **same ids** with
  the same always-in-DOM rule.
- Two motion assignments over **one** token set (§9.1).
- Roughly double the *chrome half* of the browser suite.

**What it is not is two applications:** no second engine, no second registry, no
second dispatcher, no second capability table, no second i18n space, no second
icon set, no second grid, no second formula-bar core, no second panel layer, no
second OS menu.

### 12.3 The layout axis is a second axis, not a value of `chrome`

`CHROMES = ["web", "native", "embedded"]` (`webapp/editor.core.js:949`) is a
**mount presentation** — who draws the menu bar, whose product the page is.
Layout is **which command surface is drawn**. `chrome: "native"` ×
`layout: "toolbar"` must stay reachable, because a desktop user who prefers this
chrome is exactly the user the owner's "configured based on the user's
preference" sentence names. Adding `"ribbon"` to `CHROMES` makes that state
unrepresentable.

> **The URL parameter is `?layout=`, not `?chrome=`. Rename before `UX-RIB-04`
> ships.** `docs/91` slice 04 proposes `?chrome=ribbon`, and that collides:
> `explicitMode()` already reads `PARAMS.get("chrome") === "native"` and maps it
> to `mode=desktop` (`webapp/editor.core.js:990-993`), so `?chrome=` means
> *mount*; and `URLSearchParams.get()` returns the first value, so
> `?chrome=native&chrome=ribbon` is unrepresentable — the desktop shell, whose
> window URL is statically pinned to `editor.html?chrome=native`
> (`desktop/tauri.conf.json:19`), could never ask for both. **One word today; a
> broken URL contract a shipped host already depends on, later.**

`?layout=` is allowlisted against the known layout names before it reaches a
class, exactly the way `?hide=` is filtered against `CHROME_REGIONS`
(`webapp/editor.core.js:850-853`) and `?mode=` against `MODES` (`:990-993`). An
unknown value is nobody having asked.

### 12.4 The chooser: the control, and what it reads

**`View ▸ Layout`**, immediately above `View ▸ Theme`:

```
View ▸ Layout ▸  ● Auto        Follows this device
                 ○ Ribbon      Tabbed ribbon with grouped commands
                 ○ Toolbar     Menu bar with a single toolbar row
```

**The labels are "Ribbon" and "Toolbar", with those one-line descriptions as
tooltips.** Not "Excel" and "Google Sheets": both are third-party trademarks,
`tests/browser/editor.branding.spec.mjs:57` asserts that no region of the chrome
names a product, and `ADR-026` refuses the word "Excel" in the chrome. Not
"Classic" and "Simplified" either — those two words are already taken by the
ribbon's own internal layout toggle (`docs/91` §4) and reusing them guarantees a
support conversation about which Simplified. "Ribbon" and "Toolbar" are
literally what each chrome's primary command surface is; they are honest; and
they are LibreOffice's words ("Tabbed" / "Standard Toolbar") minus the jargon.
"Auto" exists for the same reason `View ▸ Theme` has one: a user who never
chose must be able to get back to not having chosen.

Ids `view.layout.auto` / `.ribbon` / `.toolbar`, slugged from the English path
and minted **once**, into the frozen manifest — not by this workflow and the
ribbon workflow separately. No collision: `view.layout.*` is free.

**Precedent followed: `View ▸ Theme`, and it is a shipped one.** `UX-CHR-01`
moved Theme out of the Settings gear into the View menu on the reasoning that
display options belong beside Gridlines, Cell markings, Formulas-instead-of-
results, Zero values and Zoom. The Theme submenu (`webapp/editor.core.js:9776-9781`)
is the exact shape to copy: three entries, **ticked from a `current*()` function
rather than from the rendered state**, persisted and restored at boot.

**No radio group in the Settings panel, and no preview thumbnail.** Two reasons,
both from this repository's history. (a) `UX-CHR-01`'s stated lesson: *moved,
not copied — two controls for one setting drift*. (b) The gear is not reachable
on the mount that matters most: `#tb-settings` (`webapp/editor.html:95`) is
hidden by desktop chrome, by `?hide=header`, by `?mode=embedded` and by the
collapse caret, which is why `wireSettings()` has a `gearOnScreen()` test at all
(`webapp/editor.core.js:10268`+). The View menu is drawn by the OS in the
desktop shell, from `menuModel()`, so `View ▸ Layout` appears in the native
macOS/Windows menu for free.

**Read-only safety, easily missed:** `/^view\.layout/` joins `READ_ONLY_SAFE`
(`webapp/editor.core.js:4993`) beside `/^view\.zoom/`. Changing chrome writes
nothing to the document; a viewer who cannot change it is a viewer stuck in a
chrome they cannot read.

### 12.5 Storage, per mount

| mount | store | key | why |
| --- | --- | --- | --- |
| web page | `localStorage` | `oc-chrome-layout` | the convention already in use for `oc-theme`, `oc-accent`, `oc-scroll` (`webapp/editor.core.js:10384`-region) |
| desktop | the Tauri webview's own `localStorage`, same key | same | same origin-scoped store, in the app data dir; **no Rust change needed**, which matters because `desktop/src/main.rs` has no settings store today and `tauri.conf.json:19` pins the window URL statically so the shell cannot pass a preference on it |
| embedded / WOPI | **nothing is stored** | — | an embedded editor is the host's product (`webapp/embed.js:95-96` states this for the header). Writing our users' chrome preference into the host's origin is us keeping state in somebody else's page. The host supplies it |
| viewer | nothing | — | a published sheet is a link, not a session |

**Honest caveat on desktop:** `localStorage` in a webview is clearable, is not
where a desktop app's preferences belong, and is invisible to Rust — so if the
shell ever needs the layout *before* the webview boots (to size the window, or
to decide the native menu) this store cannot answer. That is a later row
(`desktop.settings.json` beside the existing app-data path, with the editor
exposing `getLayout()` so the shell can mirror it), not a reason to add a Rust
store now for a preference the webview is the only reader of.

**Read before first paint, not in `wireSettings()`.** Theme gets away with being
restored inside `wireSettings()` because a wrong theme for one frame is a flash.
A wrong layout for one frame is a 140 ↔ 144 ↔ 196 px band-height jump plus a
`resize()` and a canvas reallocation. The layout resolves at module eval, beside
`PARAMS`, where `applyModeChrome()` is already called before the wasm import
(`webapp/editor.core.js:11015`).

### 12.6 Precedence — ordered, highest first

1. **Host pin.** `sheet.layout("ribbon", { lock: true })` or
   `configure({ layout, lockLayout: true })`. Wins everything and hides the
   chooser. Implemented **as a command rule**, not as new machinery: the pin
   adds `view.layout*` to the hidden set, `applyCommandRules()`'s submenu sweep
   (`webapp/editor.selection.js:1234-1238`) then removes the now-empty "Layout"
   opener automatically, and `runCommand("view.layout.toolbar")` throws *"the
   command … is not available in this mode"* (`:1157-1160`) instead of silently
   switching. That is exactly what a host that pinned wants.
2. **`?layout=` on the URL.** Session-scoped: it wins for this page load and is
   **never written to storage**, so a link somebody hands you cannot silently
   change what you get next time. This is what the harnesses and `UX-RIB-04`
   need, and it is safe only because of the no-write clause.
3. **The user's stored preference** for this mount.
4. **Host default** — an *unlocked* `layout()` / `configure({ layout })`. A
   starting value the user may change. Deliberately below (3), the same split
   `?mode=` already makes between "a ceiling somebody set deliberately" and a
   default (`webapp/editor.core.js:986-993`).
5. **Per-mount default** (§12.7).
6. **Hard fallback: `toolbar`** — the chrome that exists today and the only one
   that survives every viewport and every pointer.

**A clamp is applied after resolution and outranks all six:** on
`(pointer: coarse)` or below the 720 px breakpoint `UX-RIB-11` already sets, the
ribbon body does not render. **The clamp must not write back to storage.** A
desktop user who once opened the document on a phone must not find their desktop
silently downgraded next Monday. **Store the choice; clamp the rendering.** This
is the single detail here most likely to be got wrong by implementing the clamp
as a preference write.

### 12.7 Per-mount defaults

| mount | default | argument |
| --- | --- | --- |
| **desktop** | **Ribbon (Simplified)** | the owner's stated priority — *"specially the Excel UI for the desktop version"*. Also the cheapest: `docs/91` §4.1 measures Simplified at 144 px against today's 162, an 18 px **gain**, so the desktop default costs grid height nowhere |
| web (standalone) | Toolbar | three arguments. **(a) Revertibility** — `docs/91` R-12's whole mitigation is that every slice up to the flip is revertible by changing one default, which only holds if the shipped default is the incumbent. **(b)** Viewport is scarcest on the web mount, where the browser has already taken its own chrome off the top. **(c)** A first-time visitor lands here, and this is the lower-floor chrome: one row, no tab model to learn. **The honest counter, stated rather than hidden:** Microsoft ships Simplified on the web and `docs/91` §4.1 calls the web "this product's primary mount". The tie is broken by revertibility, not taste, and this is the one default genuinely worth revisiting after a soak — it is a one-line change (§17 Q2) |
| embedded / WOPI | Toolbar, and it does not move without the host asking | hosts sized their embed containers against 162 px of chrome; a Classic ribbon body spends 93 px inside a card they cannot re-measure |
| viewer | Toolbar | in read-only, `applyCommandRules()` already hides the whole toolbar when no group has a live command (`webapp/editor.selection.js:1268-1276`). A ribbon would be seven tab labels above an empty band — and **the existing rule cannot reach it**, because a ribbon tab strip is not a `.tb-group`. That sweep must be parameterised per chrome (§12.1) or a viewer gets dead chrome |
| mobile / coarse pointer | Toolbar, **forced** | not a default; a refusal, per `UX-RIB-11`. So on mobile there is exactly one chrome and it is this one |

### 12.8 The moment of the switch

**No reload. Nothing lost.** And this is achievable *because of* `ADR-026`
clause (5), not in spite of it — both chromes are already required to be in the
DOM at all times so `listCommands()` cannot change size, so switching is a class
toggle on the mount root (`.oc-layout-ribbon` / `.oc-layout-toolbar`) plus one
`resize()`.

| what | survives? | how |
| --- | --- | --- |
| document, engine session, calc state | **untouched** | edits route through `EditOperation` into the session, not the DOM |
| undo history | **untouched** | nothing in the wasm session is addressed by a chrome change |
| selection | **preserved** | |
| scroll | **preserved by arithmetic, not luck** | the band height changes, so `resize()` reallocates the canvas **exactly once** after the class toggle. The anchor rule is **the top-left visible cell is preserved, not the pixel offset**, because the viewport got taller or shorter and preserving pixels would move the user's data under them. Then `ensureVisible(active)` |
| an open side panel | **survives** | `.side-panel` is a sibling of `.grid-wrap` inside `.work-area` (`webapp/editor.css:1297`), below every chrome band. Stated as a requirement with a gate, not assumed |
| open menus, flyouts, pickers | **closed first** | they are anchored to nodes about to stop having a box, and measuring a node with no box yields 0 × 0 and drops the panel in the window's corner (`webapp/editor.core.js:8806`) |
| an open cell editor | **committed, not abandoned** | the same rule `docs/91` §6.1 sets for entering the Backstage, for the same reason |
| focus | to the **new chrome's primary landmark** — the ribbon's active tab button, or this chrome's first toolbar control — **never silently to the grid** | a user who just used a menu and finds their keyboard in a cell has lost their place. `#grid-live` announces the new layout by name; no new live region |
| animation | **none** | it is a settings change, not a Ctrl+F1 collapse. There is consequently nothing to clamp under `prefers-reduced-motion` |

**Why the no-reload rule is load-bearing rather than polish:** a reload re-runs
boot, loses the draft-vs-saved distinction, and **on a live collaborative session
drops and re-resumes the OT connection** (`ADR-011`/`012`/`014`/`017`;
[61](61-COLLABORATION-RESUME.md)). "Reload to change your toolbar" inside a
shared document is precisely the failure this clause exists to prevent.

### 12.9 The integrator surface

`webapp/embed.d.ts` gains, modelled on the existing `setColorScheme(scheme)`
(`webapp/embed.js:432`-region) — an enum with an "auto" that means "follow the
mount", the same shape and the same word:

```ts
export type ChromeLayout = "ribbon" | "toolbar";
export interface LayoutOptions { lock?: boolean }

// on OpenCalcSheet:
layout(name: ChromeLayout | "auto", options?: LayoutOptions): this;
readonly layoutInUse: ChromeLayout;   // the RESOLVED value, after the clamp
```

plus `layout?: ChromeLayout | "auto"` and `lockLayout?: boolean` on
`ConfigureOptions`, and a **`layoutchange`** entry in `OpenCalcEventMap` —
hosts lay out around the embed and the band height changes by up to 56 px, so a
host that sizes its own container must be able to re-measure. `layoutInUse`
reports the **resolved** value, so a host that asked for `ribbon` on a tablet
reads `toolbar` and can trust it.

**A host that pins one chrome hides the chooser entirely** by passing
`{ lock: true }` — no separate "hide the chooser" flag, because the pin already
implies it and two flags that can disagree is one flag too many.

**No new `ChromeRegions` name** (§10.4). And one asymmetry to gate rather than
discover: `?hide=menubar` currently hides the bar's items and keeps the roster
(`webapp/editor.css:267`); under the ribbon that removes nothing visible,
because the bar is already hidden, while **under this chrome the menu bar is the
primary command surface**, so `?hide=menubar` removes a real capability. Same
region name, two very different consequences — a gate asserts both and the SDK
doc says so.

**Prerequisite that belongs to neither chrome.** `webapp/embed.d.ts:75-81`
declares `ChromeRegions` as `{ header, toolbar, formulaBar, statusbar,
sheetTabs }` while `webapp/embed.js:97-99` accepts `["header", "menubar",
"toolbar", "formulabar", "tabs", "statusbar", "localePicker"]` and throws at
`:264`. So `formulaBar` and `sheetTabs` are **declared-and-throwing**, and
`menubar` and `localePicker` are **supported-and-undeclared**. `docs/91` §6.4
says this gets its own change before ribbon work; **it should be filed as a
shared row**, because both chromes depend on it and both would otherwise inherit
it.

---

## 13. What this refuses

**1. It refuses Sheets' overflow behaviour.** Sheets collapses right-to-left one
anonymous control at a time into one unlabelled chevron, and the documented
consequence is users concluding the feature was removed
(https://support.google.com/docs/thread/174270123,
https://support.google.com/docs/thread/410076953, accessed 2026-09-09). Our
machinery folds whole **labelled** groups in an authored order and never scrolls
and never wraps. **Adopt Sheets' restraint about what is on the bar; keep our
degradation of what happens when it does not fit.**

**2. It refuses Sheets' toolbar zoom**, on selection-rule clause (d).
`#zoom-widget` (`webapp/editor.html:643`+) already shows the number permanently,
steps it, and resets it on click, and [47](47-UX-AND-FEATURE-MAP.md) ranks that
visibility the #1 daily miss. A second zoom control would be a second claim
about one fact — the same defect this chrome is removing from the save state.

**3. It refuses Sheets' demotion of Underline.** Our instrument is frequency;
Underline passes it as plainly as Bold does. Sheets' omission is a
Docs-inherited artefact rather than a frequency judgement, and copying it would
be copying the pixels instead of the rule.

**4. It refuses an Extensions menu.** We have no macros, no scripting, no
add-on host; `docs/92` names automation the largest categorical loss. A menu of
permanently disabled rows teaches a user that this product's menus lie. Its
position between Tools and Help is reserved so its arrival reorders nothing.
(§5.10)

**5. It refuses to unify the two number-format lists.** The Format menu's nine
(`webapp/editor.core.js:9870-9878`) and the toolbar's fifteen
(`#numfmt-menu`, `webapp/editor.html:405`) stay separate, because the menu's
"Thousands (#,##0)" and the toolbar's "Thousands (#,##0.00)" **both slugify to
`format.number.thousands-0`** through `commandId()`
(`webapp/editor.selection.js:1015-1023`), and one would silently shadow the
other. If a later change unifies them, both ids are assigned **by hand** and the
manifest gate asserts uniqueness.

**6. It refuses to clean up three duplicate-verb pairs.** `view.settings` /
`tools.settings` / `toolbar.settings` (one panel, three ids);
`insert.pivottable` / `data.pivottable-fields` (one dialog); `toolbar.filter` /
`data.filter`. They are cheap and their ids are load-bearing. This chrome adds
*routes*, never ids.

**7. It refuses a suggestion mode.** **Google Sheets does not have one.** The
Editing / Suggesting / Viewing selector is a Google **Docs** feature
(https://support.google.com/docs/answer/6033474 is a Docs-only page, accessed
2026-09-09), corroborated by standing community threads asking why it cannot be
found in Sheets (https://support.google.com/docs/thread/14593245,
https://support.google.com/docs/thread/28604653, accessed 2026-09-09). This is
an argument from absence and is marked as one. What Sheets substitutes is a
three-part review stack — comments with assignment, right-click ▸ *Show edit
history* on a cell, and version history with per-person colour attribution
(https://support.google.com/docs/answer/190843, accessed 2026-09-09). **We build
the substitute, not the mode.** §7 covers the first and the third; the second is
a proposed row of its own, and it is a place this chrome can beat the product it
imitates, because `cellAuthor()` / `session_cell_author` already exist
(`webapp/editor.core.js:5203-5206`) and
[89](89-CHANGE-ATTRIBUTION-AND-TRACKING.md) already holds the data model.
**Shipping an empty mode selector to match a screenshot would be shipping a
lie.**

**8. It refuses Explore / Ask Gemini.** Sheets has two generations of one slot
(a bottom-right corner sheet and a top-right assistant panel) and we have
neither generation's substance. Drawing the button first is drawing a door with
no room behind it. If it ever lands, the honest home is a host-provided seam
([78](78-HOST-CAPABILITY-SEAMS.md)), not a chrome feature.

**9. It refuses Insert ▸ Image and Insert ▸ Drawing**, and the reason is
specific rather than general: half the capability exists and **it is the wrong
half**. `docs/92` R-2 corrects the record — reading images is engine-capable,
writing is not; there is no `session_add_image`, and the PDF backend refuses
`Image` outright (`crates/casual-calc-render/src/pdf.rs:883-889`). An Insert ▸
Image whose output the export path deletes is silent data loss with a menu item
in front of it.

**10. It refuses slicers, timelines, smart chips, emoji, checkbox, data
connectors, named functions, randomise range and data extraction** — Sheets menu
items with no engine behind them here. Not drawn in any state; each is a
candidate tracker row, not a greyed line.

**11. It refuses to thin the pivot panel toward Sheets'.** Sheets' pivot surface
is materially thinner than Excel's — no calculated measures, thinner formatting
— and ours (`webapp/editor.pivot.js`) already exceeds it. **A Sheets-shaped
chrome must carry more than Sheets does in the two places where Sheets is the
weaker product**, which are the pivot panel and Format ▸ Cell format. Copying
the shape is not copying the ceiling.

**12. It refuses to touch the formula bar, the status bar's `#tb-status`, or
the sheet strip's composition.** Three shipped decisions stand: one flat formula
bar with one seam (`UX-CHR-08`), engine state in the status bar (`UX-CHR-03`),
one sheet row with the rail pinned left (`UX-CHR-07`).

**13. It refuses Sheets' skin.** No traced, screenshot-derived or
redrawn-from-memory Google glyph. **No Google green, no Sheets wordmark, no
product name anywhere in the chrome.** Large branded surfaces fill with
`--oc-accent-color`, the one host-swappable token.
`tests/browser/editor.branding.spec.mjs:57` already asserts it with
`?brand=Ledgerly`.

**14. It refuses a measurement loop for the collapse ladder.** §6.4 replaces
`fits()` with seven named widths. The collapse *point* is measured by a gate;
the collapse *order* never is.

**15. It refuses to fix four adjacent defects inside this work.** Each is a row,
not a chrome change: the `embed.d.ts` / `embed.js` `ChromeRegions` mismatch
(§12.9); the two missing `oc-btn` classes at `webapp/editor.dialogs.js:1832` and
`:1834` (§7.3); the stale comment at `webapp/editor.core.js:1352` claiming
`canShare` is false in every preset when `:955` and `:958` set it true; and
[82](82-UX-VISUAL-AUDIT.md) being a generated artifact that CI does not
regenerate — `tests/browser/ux-visual-audit.mjs` exists and no workflow
references it, so the document has been wrong about `window.prompt` since the
row-height/column-width fix landed. **Per `CLAUDE.md`, a document stating a
contract the code does not keep is a row, not a prose fix.**

---

## 14. Risks

**R-1 — Command-id churn, through a regroup. Critical.** `commandId()` mints ids
by slugifying the English menu path (`webapp/editor.selection.js:1015-1023`).
This chrome moves six verbs to Sheets' menu positions — Settings (File vs
Tools), Named ranges (Data vs Tools), Protection (Data vs Format), Group (View
vs Data), row/column deletion (Edit vs Insert), Rotation (Format vs
toolbar-only). If any owner *moves*, its id is re-derived, and
`CAPABILITY_COMMANDS` (`webapp/editor.core.js:1336-1358`) and `READ_ONLY_SAFE`
(`:4993-5008`) both match **by regex** — so moving Calculation into File ▸
Settings the way Sheets does takes recalculation away from every viewer, and
regrouping Download un-gates saving. A UI change becoming a security defect,
with nothing red anywhere. *Mitigation:* §5.1's proxy rule, absolutely; the
frozen manifest with a static gate **before any chrome markup is written**; and
the builder never calling `commandId()` on a Sheets label path — the menu is
**data carrying explicit ids**, the way `MENUS` (`webapp/editor.core.js:9611`)
is data consumed by `buildMenuBar()`, with the id declared rather than derived.

**R-2 — A proxy escaping `applyCommandRules()`. High.** §12.1. Red the moment
the first proxy ships; the gate is *hide `file.download.csv-csv`, then assert no
clickable node for that id exists anywhere in the DOM*.

**R-3 — `listCommands()` shrinking as the user clicks or resizes. High.** It
reads the live DOM (`webapp/editor.selection.js:1040-1046`), so a lazily-rendered
panel, a chip flyout that clones instead of moving, or a contextual surface that
renders on selection silently deletes published SDK surface. `docs/91` §6.3's
Constraint B is the sharp version and is adopted verbatim: a snapshot must be
identical at boot, with a chart selected, with a table selected, with a pivot
selected, with a panel open, **and at every breakpoint in §6.4's ladder**.

**R-4 — Four existing chrome gates degrading to vacuous passes. Critical, and
worse under two layouts.** `docs/91` §9 R-2 names them:
`editor.toolbar-inventory.spec.mjs:26` sweeps `.tb-collapsed` and stops
enforcing the width budget; `editor.chrome-reachability.spec.mjs:47` sweeps
`.toolbar .tb-btn` and an empty array passes; `editor.branding.spec.mjs:64`
reads `#menubar` text with `?? ""`; `editor.native-chrome.spec.mjs:735` calls
`toBeHidden()` on a detached node. With two layouts the failure is worse than
red — a suite run under one layout passes vacuously for the other and the team
believes it is covered on both. *Mitigation:* every chrome gate carries **two**
guards — a vacuity guard (assert the sweep found something; the pattern
`editor.native-chrome.spec.mjs:405` already demonstrates) **and** a layout
assertion. Each replacement lands in the same commit as the change that empties
the old one. **Prove the parameterisation is real by writing one deliberately
layout-specific assertion and watching it fail under the other layout.**

**R-5 — The presence relocation silently dropping the roster. High.**
`.oc-hide-menubar .menubar > *:not(.presence)` (`webapp/editor.css:267`) and
`tests/browser/editor.chrome-regions.spec.mjs:43` exist because a host that hid
the menus once lost the ability to see who else was in the document. Moving
`#presence` to the app bar re-opens that hole from the other side: now
`?hide=header` could take it. *Mitigation:* the reversible two-step through
`chromeHome` / `placeNativeChrome` (`webapp/editor.core.js:1310`), and a gate
that asserts the roster survives **both** `hide=menubar` and `hide=header` in
this chrome.

**R-6 — The panel slide costing grid frames. High.** `resize()` reallocates the
canvas and calls `draw()` (`webapp/editor.geometry.js:229-238`), and the stated
budget is 60 fps at 1M cells with < 50 ms recalc. A 300 ms panel slide that
resizes per frame is the most visible possible violation of it. *Mitigation:*
§9 item 2's overlay-then-reflow contract, `.oc-grid-busy` suppressing chrome
transitions while a frame is pending, and a gate that counts `resize()` calls
across one open and one close and asserts **exactly one each**.

**R-7 — The two chromes disagreeing on a shared value. High, and live today.**
Three known: `--oc-pressed-color` (`docs/91` §2.2 defines it as
`var(--oc-surface-color)`, which is this chrome's hover fill, so a press would
be invisible here — §8.4); the focus-ring `box-shadow` transitioning under
`.tb-btn`'s shipped `.1s` (§9 item 9), which both designs specify and neither
noticed; and `docs/91` §9 R-3's four retaken assertions, which are written
against the ribbon's numbers and **must be parameterised by chrome** — two
chromes need different `.toolbar` heights (40 here, 41 Simplified), different
band totals (140 vs 144) and different coarse-pointer growth. *Mitigation:*
resolve all three **before either chrome lands**; a retake written against one
chrome fails the other.

**R-8 — A command added to one chrome and forgotten in the other. High.** The
most likely long-run failure, and the one that degrades slowly: a verb reachable
in the ribbon and invisible here for months, discovered by a user, not by CI.
*Mitigation:* the manifest gate asserts **set equality both ways, per layout** —
an id in the registry that no layout places is a build failure in both, and a
layout naming an id the registry lacks is a build failure. The one-way version
passes while a layout holds a stale id.

**R-9 — The id space being minted twice. High.** ~60 verbs live only in context
menus and carry no id (§1.7); this chrome needs ~21 of them and the ribbon needs
the rest. If both mint independently, one verb ends up with two ids and the
capability regexes match exactly one of them. *Mitigation:* §15's wave rule —
whichever programme ships the manifest mints them all, once.

**R-10 — The test matrix and the visual-audit surface doubling. High.**
`browser-smoke` is one serial job of 97 spec files. Naively running everything
under both layouts doubles the long pole — and the local-flake temptation is
exactly the one that once exhausted this machine's ephemeral port range across
eight re-runs. *Mitigation:* parameterise, never copy (one spec with
`for (const layout of LAYOUTS)`, so an assertion cannot be added to one and not
the other); name the ~60 chrome-agnostic specs as single-run in the row; prefer
a few gates driving many widths in one page load; **state the measured added
runtime as a number in the row before the second layout lands**; and after a
second unexplained local failure, push and let CI answer.

**R-11 — i18n divergence. High.** Two `chrome.*` sub-spaces would double every
host's catalogue and guarantee a permanently half-translated product. And the
length gate is *harder* on this side: a single fixed toolbar row has less slack
for a 1.4× German string than a ribbon's tab structure does. *Mitigation:* one
space (§12.2); the 1.4× clipping gate run under both layouts, asserting nothing
clips, no group wraps and no band height changes.

**R-12 — Escape destroying a draft, which ships today. Medium, and it is a
correctness defect the panel programme cannot ship on top of.**
`webapp/editor.core.js:10245-10248`. *Mitigation:* §11.5, with a gate that is
red against the current tree.

**R-13 — The coarse-pointer clamp written back as a preference. Medium.**
§12.6. *Gate:* set the preference to `ribbon`, boot coarse, assert the resolved
layout is `toolbar` **and** the stored value is still `ribbon`; then boot fine
and assert the ribbon returns.

**R-14 — `?chrome=` overloaded to carry two axes. Medium now, High the day it
ships.** §12.3. One word today; a breaking URL contract a shipped host and four
spec files already depend on, later.

**R-15 — The tracker-id gate's known blind spot. Medium.** `docs/91` §9 R-15
records that the ADR-status gate filters tracker rows with `^[A-Z]{2,6}-\d+$` —
a two-segment id — so three-segment ids are silently skipped. **Every row
proposed in §16 is three-segment.** It is a row of its own and it is filed
before any `UX-GS` row closes. Separately, run `tools/check-tracker-ids.py`
before any row here is created: five id collisions are already on record and
`FID-13` named two unrelated changes for weeks.

**R-16 — The command count entering a gate before anyone has measured it.
Medium.** `webapp/docs.html:353` advertises 189, `docs/91` §3.12 asserts 188,
and neither is a measurement. *Mitigation:* it does not enter a gate or a
document until `listCommands().length` has been read from a booted editor.

---

## 15. The work

Twelve slices. Ranked by what unblocks what, not by visible progress: the first
two produce no pixels and everything after them is unsafe without them.

| # | slice | size | depends on | acceptance |
| --- | --- | --- | --- | --- |
| **01** | **Decide it in writing.** This note, an ADR recording that layout is a second axis orthogonal to `chrome`, that neither chrome owns an id, and the per-mount default table; the `?chrome=ribbon` → `?layout=` rename ruled for `docs/91` slice 04; `view.layout.*` reserved | S — 1 day, docs only | — | the repository-policy doc gates; `tools/check-tracker-ids.py` clean; the ADR cross-referencing `ADR-026` rather than superseding it |
| **02** | **The command spine.** Owners move out of `.toolbar` markup into one hidden chrome-neutral registry; the `[id^='tb-']` boot sweep becomes a read of the frozen manifest; `anchorFor(id)` resolves the *visible* proxy; `applyCommandRules()` gains a `[data-oc-proxy]` pass and manifest-supplied container selectors. **No visible change** | L | `UX-RIB-02` (the frozen manifest — this slice **consumes** it and must not re-derive it), 01 | `listCommands()` byte-identical before and after; `menuModel()` identical; the desktop readiness poll still flips; a static gate that no two nodes carry the same `data-oc-command`; the proxy-rules gate |
| **03** | **Mint the ungoverned ids, jointly.** §5.11's 21, plus the ribbon's share of the ~40 context-menu verbs, in one manifest edit | M | 02 | every minted id resolves in `listCommands()`; every context-menu verb reachable by pointer is in `applyCommandRules()`; **set equality both ways** |
| **04** | **The layout axis.** `?layout=` allowlisted at module eval; `oc-chrome-layout` read before first paint; the six-rank ladder; the clamp applied after resolution and never written back; `currentLayout()`/`applyLayout()` modelled on `currentTheme()`/`applyTheme()`; an `.oc-layout-*` class. **Ships with only today's toolbar, so it is a verifiable no-op** | M | 01 | `?layout=nonsense` resolves to the mount default and adds no class; a stored preference survives reload; `?layout=` never writes storage; the coarse-pointer clamp gate (R-13) |
| **05** | **The chooser.** `View ▸ Layout`, four entries, ticked from `currentLayout()`; `/^view\.layout/` into `READ_ONLY_SAFE`; the host pin as a command rule | S-M | 04, and 02 for the manifest | the tick follows the **choice**, not the rendering (set Auto on a system that renders ribbon, assert Auto is ticked); a pinned host has no `view.layout` node and `runCommand` throws; the entry appears in `menuModel()` under `?chrome=native` |
| **06** | **The switch contract.** §12.8, landing with a **stub second layout** (an empty band behind `?layout=ribbon`) so it is testable before either real chrome exists, and re-run for real at `UX-RIB-04` | M | 04 | the place-preservation gate: scroll to row 5000, select F5000, open the CF panel, begin typing, switch — active sheet, active cell, top-left visible cell, open panel, undo depth and committed value unchanged; `resize()` called once; no navigation; focus on the new landmark |
| **07** | **The panel contract.** Width clamp, non-modality, **scoped Escape**, focus on open and return to invoker, the responsive floor, the `aria-modal` switch below 1024 | M | 04 | at 800 × 600 with a panel open, `#grid` client width ≥ 480; type in the CF value field, press Escape, panel still visible and text retained (**red against today's tree**); `resize()` exactly once per open and per close |
| **08** | **The Sheets band stack**, behind `?layout=toolbar` beside the existing chrome: app bar, menu bar, the tonal toolbar container, the authored ladder, the state and motion vocabularies, compact controls on Ctrl+F1. Reuses `.oc-hide-*` (`webapp/editor.css:253-268`) | L — every mechanism is decided here | 02, 03, 04, 06, 07, and `UX-RIB-03` for icons (stubbed glyphs are fine) | region heights measured against §3.2's arithmetic; the disclosure-caret biconditional; the collapse ladder measured at 2560/1920/1440/1366/1280/1180/1120/1024/900/768; **all existing specs still green**, because today's toolbar is untouched |
| **09** | **The snackbar and the feedback relocation.** The eligibility rule; dismiss on any local edit; predicate-driven menu disabling with reasons; `#tb-status` narrowed and given `role="status"` | M | 08 | delete a chart — no dialog, a snackbar, and Undo restores it; make a local edit while it is showing and assert it is gone; `#tb-status` no longer receives a completed-action message |
| **10** | **The panel conversions**, in dependency order: CF list+edit with `docs/92` R-8's four operators; sort with C-8's unlimited keys; cell format with R-14's shrink-to-fit; named ranges; protected ranges; split-text; import report; the link bubble | L — the assignment is the work | 07, 08 | for each: the modal is gone, the panel is non-modal, and **a round-trip test that a workbook carrying the previously-unauthorable state survives an open-edit-save cycle** |
| **11** | **The collaboration strip.** Presence relocated and populating; the Share pill; one save state, event-driven; comment attribution from the roster; the comment thread list; Ctrl+Alt+M and Ctrl+Alt+Shift+A | M | 08 | the roster survives `hide=menubar` **and** `hide=header` in this chrome; `#doc-state` announces through `role="status"`; a comment written in a session is attributed to the roster name with no name field on screen |
| **12** | **The tool finder, and the shortcut sheet rebuilt from `menuModel()`.** Alt+/, Ctrl+K, Ctrl+/ | M | 03, 08 | every `listCommands()` id is findable by its label; a disabled command is listed **with its reason**; the shortcut sheet's rows are derived, not literal — delete a menu item and assert its row disappears |

### The wave rule, against `UX-RIB-01..13`

Per [67](67-REPOSITORY-REMEDIATION-PLAN.md), items run in parallel only when
they cannot touch the same invariant.

**Forbidden pairs — and there are many, because the two programmes share a
spine.**

- **Any `UX-GS` slice with `UX-RIB-02`, `04`, `09`, `12` or `13`.** Those five
  edit `webapp/editor.selection.js`, the command region of
  `webapp/editor.core.js`, `webapp/editor.css`'s `.oc-hide-*` block, or
  `webapp/embed.js` / `embed.d.ts` — the same files every slice above depends
  on. That is most of the ribbon programme.
- **02 with anything.** It edits `webapp/editor.selection.js`, which every other
  slice depends on. It runs alone. This is `UX-RIB-02`'s own exclusion (e),
  inherited.
- **03 with `UX-RIB-06`.** Both mint ids. One worker, one manifest edit, or one
  verb gets two ids and the capability regexes match exactly one of them.
- **05 with `UX-RIB-09`.** Both touch the Alt / mnemonic space, and `UX-RIB-09`
  already moves `Alt+V`.
- **06 with `UX-RIB-05`.** Both change a band height and both must call
  `resize()` exactly once at `transitionend`; two workers will each write their
  own call.
- **08 with `UX-RIB-04`.** The same control primitives, the same width budget,
  the same stylesheet. This is the conflict `docs/88` §9 already names — *one
  worker in sequence, not two in parallel*.
- **11 with `UX-RIB-08`.** Both relocate the collaborator roster. Doing it twice
  is how it gets folded away a third time.

**Safe pairs, verified:** 01 with `UX-RIB-01` or `UX-RIB-03` (docs against docs,
docs against new asset files); 04 with `UX-RIB-03`; 10 with 12 (different files,
different invariants) **after** 08 freezes the primitives.

**And the schedule cost, said plainly rather than discovered: 01, 02 and 03 all
precede `UX-RIB-04`.** The ribbon programme is delayed by roughly one slice of
shared foundation. Paying it later costs the id space, and the id space is the
one thing here that cannot be repaired after the fact.

`git status --porcelain` after every round; strays have happened. And the
orchestrator verifies by **running** — revert the fix, watch the test go red,
restore, re-run. A worker's report is not evidence.

---

## 16. Proposed tracker rows

**Ids are `<assign>` and must be minted by `tools/check-tracker-ids.py` at
filing time**, under the prefix **`UX-GS-`**. `UX-RIB-01..13` are taken. Five id
collisions are already on record in this repository and `FID-13` named two
unrelated changes for weeks, so the id space is read before a row is added, not
after. These rows are **not** written to [14](14-EXECUTION-TRACKER.md) by this
round — another workflow holds that file. Rows are in §15's slice order; the
last three are the adjacent defects §13.15 refuses to fix inside the chrome
work. `docs/00-README.md` also needs an index row for this document at filing
time, for `tools/check-doc-index.py`.

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| `<assign>` | The Sheets chrome is decided in writing, or its 30 toolbar seats get chosen incrementally by whoever is holding the file | Designed | P2 | The product owner ruled the editor gets two selectable chromes — *"we need both UIs, which can be configured based on the user's preference"* — superseding [88](88-EDITOR-CHROME-COMPOSITION.md) §8.1 in the same act that `ADR-026` superseded it for the ribbon. This note is that decision: the selection rule (§6.1), the menu assignment (§5), the panel set (§7), the motion vocabulary (§9) and the chooser (§12). It also rules `?chrome=ribbon` → `?layout=` for `docs/91` slice 04 and reserves `view.layout.*`, both of which are one-word changes now and breaking contracts later. Blocks every row below | The repository-policy doc gates; `tools/check-tracker-ids.py` clean; a new ADR reaching a settled status and cross-referencing `ADR-026` rather than superseding it |
| `<assign>` | Two chromes both claiming `id="tb-bold"` is unrepresentable, and the invisible one silently receives every programmatic click | Designed | P1 | The boot sweep stamps `data-oc-command` onto every `[id^='tb-']` node (`webapp/editor.core.js:11019-11021`), so ~30 published ids are owned by toolbar markup — and `docs/91` §3.3 assigns the same ids to ribbon controls. DOM ids are unique. Worse, `runCommand` dispatches to `qsa(...)[0]`, the first node in document order (`webapp/editor.selection.js:1146-1147`), so with both chromes present the hidden one receives the click and any popover it opens anchors to a 0 × 0 node (`:8806`). The owners move into one hidden chrome-neutral registry; both chromes become 100 % proxies; `applyCommandRules()` gains a `[data-oc-proxy]` pass, because a proxy for a hidden command is currently visible, clickable, and clicks the hidden owner — which `runCommand` refuses and a mouse does not. No visible change | `listCommands()` byte-identical before and after; `menuModel()` identical; the desktop readiness poll (`desktop/src/main.rs:927`) still flips; a static gate asserting no two nodes carry the same `data-oc-command`; and the proxy-rules gate — `setCommandRules({ hidden: ["file.download.csv-csv"] })`, then assert **no clickable node for that id exists anywhere in the DOM** |
| `<assign>` | ~60 verbs exist only in context menus and are invisible to every governance mechanism, and two chromes will mint their ids twice | Designed | P2 | Paste Special and its four variants (`webapp/editor.dialogs.js:2479-2485`), Insert/Delete Cells with shift (`:2523-2527`), Row Height / Column Width / AutoFit (`headerMenu`, `:2792`), the sheet verbs (`sheetMenu`, `:2295`), `#fx-insert` (`webapp/editor.html:502`), `#name-box-list` (`:498`), the three `.tb-align` buttons (`:316`, `:319`, `:322`) and `autoSum()` (`webapp/editor.selection.js:1320`, which has **no DOM node at all**). None carries a command id, so none is visible to `listCommands()`, `applyCommandRules()`, the read-only whitelist, the capability gates or the OS menu. This chrome needs ~21 of them (§5.11) and the ribbon needs the rest; if both mint independently one verb gets two ids and the capability regexes match exactly one | Every minted id resolves in `listCommands()`; every context-menu verb reachable by pointer is inside `applyCommandRules()`; **set equality both ways** between the manifest and a booted editor, so a stale id in either direction is a build failure |
| `<assign>` | Chrome layout is a second axis and `?chrome=` cannot carry it | Designed | P2 | `CHROMES` (`webapp/editor.core.js:949`) is a mount presentation; layout is which command surface is drawn, and `chrome: "native" × layout: "toolbar"` must stay representable. `explicitMode()` already reads `PARAMS.get("chrome") === "native"` as a mount alias (`:990-993`) and `URLSearchParams.get()` returns the first value, so the desktop shell — whose URL is pinned to `editor.html?chrome=native` in `desktop/tauri.conf.json:19` — could never ask for both. Adds `?layout=` allowlisted at module eval beside `?hide=` (`:850-853`), `oc-chrome-layout` in `localStorage` read **before first paint** (a wrong layout for one frame is a band-height jump plus a canvas reallocation, unlike a wrong theme), the six-rank precedence ladder, and the coarse-pointer clamp applied **after** resolution and never written back. Ships with only today's toolbar, so it is a verifiable no-op | `?layout=nonsense` resolves to the per-mount default and adds no class; a stored preference survives reload; `?layout=` never writes storage; and the clamp gate — store `ribbon`, boot under `(pointer: coarse)`, assert the resolved layout is `toolbar` **and** the stored value is still `ribbon`, then boot fine and assert the ribbon returns |
| `<assign>` | A chrome the user cannot choose is not a preference, and a chooser in the Settings gear is unreachable on the desktop | Designed | P2 | `View ▸ Layout ▸ Auto · Ribbon · Toolbar`, above `View ▸ Theme`, following the shipped `UX-CHR-01` precedent exactly: display options live in View, the tick reads the **choice** and not the rendering, the value persists. Not the Settings panel — `#tb-settings` (`webapp/editor.html:95`) is hidden by desktop chrome, by `?hide=header`, by `?mode=embedded` and by the collapse caret, which is why `wireSettings()` has a `gearOnScreen()` test at all. Labels are "Ribbon" and "Toolbar", never the competitors' product names, which `tests/browser/editor.branding.spec.mjs:57` forbids. `/^view\.layout/` joins `READ_ONLY_SAFE` (`webapp/editor.core.js:4993`) beside `/^view\.zoom/`, because changing chrome writes nothing to the document. A host pin is expressed as a command rule, so the empty submenu opener disappears through the cascade already at `webapp/editor.selection.js:1234-1238` | The tick follows the choice, not the rendering — set Auto on a system that renders ribbon and assert Auto is ticked; a pinned host has no `view.layout` node and `runCommand("view.layout.toolbar")` throws *"not available in this mode"*; the entry appears in `menuModel()` under `?chrome=native`, so it reaches the OS menu |
| `<assign>` | Switching chrome must not lose the user's place, and must not reload a live collaborative session | Designed | P2 | A reload re-runs boot, loses the draft-vs-saved distinction, and drops and re-resumes the OT connection (`ADR-011`/`012`/`014`/`017`), so "reload to change your toolbar" inside a shared document is the failure this contract exists to prevent. Switching is a class toggle plus one `resize()`. Overlays close first, because they anchor to nodes about to stop having a box and a 0 × 0 measurement drops a panel in the window's corner (`webapp/editor.core.js:8806`); an open cell editor is **committed, not abandoned**; the **top-left visible cell** is preserved rather than the pixel offset, because the viewport changed height; focus goes to the new chrome's primary landmark and never silently to the grid. Lands with a stub second layout so it is testable before either real chrome exists | Scroll to row 5000, select F5000, open the CF panel, begin typing in a cell, switch layout — assert active sheet, active cell, top-left visible cell, open panel, undo depth and committed cell value all unchanged; `resize()` called **exactly once**; no navigation occurred; focus is on the new landmark |
| `<assign>` | Escape inside a panel field destroys the rule being written, and the panel squeezes the grid to 74 px on a phone | Designed | P1 | Two defects that ship today and that a panel-based chrome cannot ship on top of. (1) The close handler fires on any `Escape` while `activePanel` is set with **no check on `e.target`** (`webapp/editor.core.js:10245-10248`), and `openPanel` clears the body on the next open (`:5160-5161`), so a half-written conditional-formatting rule or comment draft is unrecoverable. The guard exists at exactly two sites — `webapp/editor.dialogs.js:1090` and `webapp/editor.core.js:4449` — and was never generalised, **which is what made the defect invisible to anyone who tested the two panels that happen to work**. (2) `.side-panel { flex: 0 0 316px }` (`webapp/editor.css:1301`) at every width, with no `@media` rule in the file touching it, leaves the grid 484 px at 800, 244 at 560 and 74 at 390 | Type into the CF value field, press Escape, assert `#side-panel` is still visible and the field retains its text — **red against the current tree**; at 800 × 600 with a panel open assert `#grid` client width ≥ 480; and `resize()` called exactly once per open and once per close |
| `<assign>` | The Sheets band stack, or the chrome gets built out of whatever the first markup PR happened to draw | Designed | P2 | App bar 36, menu bar 28, toolbar 40 (a 32 px tonal container inset 4), formula bar unchanged — 140 px above the grid against today's 162, the ribbon's 144 Simplified and 196 Classic. One elevation, inverted: the container lifts once and the controls inside it tint, which is why three of the five hairlines can go. The collapse ladder is **authored at seven named widths**, not computed — today's first fold fires at a width nobody chose (`docs/88:110-111`), and the computed budget here is 1305 px against our current 1463. Ships behind `?layout=toolbar` beside the existing chrome, reusing `.oc-hide-*` (`webapp/editor.css:253-268`), with compact controls on Ctrl+F1 rather than Sheets' Ctrl+Shift+F, which is already Format Cells (`webapp/editor.core.js:8470`) | Region heights measured against §3.2's arithmetic; the disclosure-caret **biconditional** — every `.tb-btn[aria-haspopup="true"]` has non-empty `::after` content and every `.tb-btn` without the attribute has none; the ladder measured at 2560/1920/1440/1366/1280/1180/1120/1024/900/768 with the fold order asserted **in sequence**; every existing spec still green, because today's toolbar is untouched |
| `<assign>` | Two reversible deletions ask before acting while 37 completed actions report themselves 700 px away in an element with no `role` | Designed | P2 | `confirmModal("Delete chart", …)` (`webapp/editor.core.js:4535`) and `confirmModal("Delete pivot table", …)` (`:4815`) block in front of operations the engine proves reversible — `session_delete_chart` routes through `EditOperation` (`crates/casual-calc-wasm/src/objects.rs:689` → `axis.rs:1041`), proven by `undoing_a_chart_deletion_restores_its_retained_bytes` (`crates/casual-calc-wasm/src/lib.rs:2189`) — while success lands in `#tb-status` (`webapp/editor.html:597`), a 34 px strip with no `role` and no `aria-live`. The reversibility budget is spent exactly backwards. Adds the snackbar with the eligibility rule *(routes through `EditOperation` ⇒ snackbar; loss outside the model ⇒ confirm)*, dismissing on **any** subsequent local edit because undo is per-user and travels to peers ([69](69-COLLABORATIVE-UNDO-POLICY.md):3-7) and a stale Undo would reverse the wrong operation. Also disables commands that cannot run, with the reason in `title`, using the predicate slot `MENUS` already supports (`webapp/editor.core.js:9831-9832`) | Delete a chart: no dialog, a snackbar appears, Undo restores it and `session_versions`-independent state is intact; make a local edit while the snackbar is showing and assert it is gone; `#tb-status` no longer receives a completed-action message and gains `role="status"` |
| `<assign>` | Nine modals cover the data they configure, and four of them hide engine capability the editor cannot otherwise reach | Designed | P2 | `manageCfRules()` (`webapp/editor.dialogs.js:89-138`) can list rules and cannot edit them; `buildCfPanel` (`:1528`) can edit one and cannot list — two doors 44 lines apart in the same menu (`webapp/editor.core.js:9837`, `:9881`) with labels differing by one word. `sortDialog()` (`:590`), `textToColumnsDialog()` (`:2159`), `tableDialog()` (`:1867`), `hyperlinkDialog()` (`:1788`), `openNameManager()` (`:2244`), `formatCellsDialog()` (`:141`) and `pasteSpecialDialog()` (`:2090`) all cover their subject. Converting them reaches four [92](92-COMPETITIVE-GAP-ANALYSIS.md) §5 rows in the same act: **R-8**'s four conditional-format operators, which exist and evaluate in `CfRule` (`crates/casual-calc-model/src/sheet.rs:914-921`) and are absent from the operator array at `webapp/editor.dialogs.js:1531-1546`; **C-8**'s sort keys, capped by a literal at `:641` while `session_sort_range_multi` takes a vector (`crates/casual-calc-wasm/src/structural.rs:415-423`); **R-14** shrink-to-fit, with a wire byte at `crates/casual-calc-wasm/src/axis.rs:815-817` and no control anywhere; and **R-3** chart grouping. Two of those mean a workbook opened and edited here **silently narrows** | Per surface: the modal is gone and the panel is non-modal. And the one that matters — **a round-trip test that a workbook carrying the previously-unauthorable state (a `NotBetween` rule, a four-key sort, a shrink-to-fit cell) survives an open-edit-save cycle unchanged**, red before the fix |
| `<assign>` | The editor runs a collaboration server and the chrome does not say so | Designed | P2 | `#presence` is `hidden` until `collabSession` is truthy (`webapp/editor.presence.js:260-263`) and lives inside `#menubar` (`webapp/editor.html:181`); `Share…` is the tenth item of the File menu (`webapp/editor.core.js:9679`) — a circular dependency in which the entry point to the collaborative product is invisible until collaboration is already happening. Meanwhile the comment panel asks the user to type their name into a `localStorage` field (`webapp/editor.core.js:5545-5549`) while the roster's name for that same person is already painted on their live cursor (`webapp/editor.paint.js:904`), so one person is two identities in one document. And three indicators disagree about whether the work is safe (`#doc-state` on a 250 ms poll at `:9188`/`:9193`, `#autosave-state` hidden while healthy at `webapp/editor.drafts.js:514-521`, `.bottom-status` permanently "nothing uploaded" at `webapp/editor.html:609`). The relocation of `#presence` is the reversible two-step through `chromeHome`/`placeNativeChrome` (`webapp/editor.core.js:1310`), never a markup move | The roster is visible under **both** `hide=menubar` and `hide=header` in this chrome — `tests/browser/editor.chrome-regions.spec.mjs:43` re-pointed, never deleted; `#doc-state` announces through `role="status" aria-live="polite"`; a comment written during a session is attributed to the roster name with **no name field on screen** |
| `<assign>` | A thirty-control toolbar is only defensible if everything left off it is one keystroke away | Designed | P2 | Without a search route, editorial restraint is amputation and every control removed comes back next year — which is how a toolbar grows to a ribbon. The registry already exists and is already derived from the live DOM rather than a literal: `menuModel()` (`webapp/editor.selection.js:1065`) carries every leaf's id, label, enabled state and menu path, so read-only mode and host capability rules are honoured for free and the list cannot drift from the menus. `Alt+/` and `Ctrl+K` are both free (the Alt handler looks up `menuMnemonics.get("/")`, undefined on a letter-keyed map; `grep 'key === "/"'` returns nothing). Disabled commands are shown **with their reason** rather than hidden, because "I cannot find it" and "I cannot use it here" are different problems. `Ctrl+/` rebuilds the shortcut sheet from the same model, retiring the 17 hand-written pairs at `webapp/editor.core.js:9569-9588` that are maintained separately from ~129 menu leaves. [92](92-COMPETITIVE-GAP-ANALYSIS.md) X-8 rates this *small* | Every `listCommands()` id is findable by its label; a disabled command is listed **with its reason**; and the shortcut sheet is derived, not literal — delete a menu item and assert its row disappears from `Ctrl+/` |
| `<assign>` | `ChromeRegions` declares two names that throw and omits two that work, and both chromes would inherit it | Designed | P2 | `webapp/embed.d.ts:75-81` declares `{ header, toolbar, formulaBar, statusbar, sheetTabs }` while `webapp/embed.js:97-99` accepts `["header","menubar","toolbar","formulabar","tabs","statusbar","localePicker"]` and throws on anything else at `:264`. So `formulaBar` and `sheetTabs` are declared-and-throwing and `menubar` and `localePicker` are supported-and-undeclared. It survived because `sdk/types/consumer.ts` exercises only `toolbar` and `statusbar`. `docs/91` §6.4 rules it precedes ribbon work; filing it as a **shared** row is the correction, since both chromes depend on it | The `sdk-types` gate exercising **all seven** accepted names and none that throw |
| `<assign>` | The two chromes disagree on three shared values, and each disagreement is invisible until both have shipped | Designed | P2 | (1) `--oc-pressed-color`: `docs/91` §2.2 defines it as `var(--oc-surface-color)`, which **is** the Sheets chrome's hover fill, so a press would be invisible there; the derived `color-mix(in srgb, var(--oc-text-color) 9%, var(--oc-background-color))` darkens correctly against both. (2) Both designs specify the focus ring as `outline` **plus** a 4 px `--oc-accent-ring` box-shadow, while `.tb-btn` ships `transition: … box-shadow .1s` (`webapp/editor.css:765`) — so the ring **fades in**, and a ring absent for the first frame after a keypress is worse than one not animated. One line: `.tb-btn:focus-visible { transition: none; }`. (3) `docs/91` §9 R-3's four retaken assertions are written against ribbon numbers; two chromes need different `.toolbar` heights (40 vs 41), different band totals (140 vs 144) and different coarse-pointer growth, so the retakes must be **parameterised by chrome** or each fails the other | The three fixes land in one change; the four retaken assertions each run under both layouts and each carries a layout assertion, so a suite run under one layout cannot pass vacuously for the other |
| `<assign>` | [82](82-UX-VISUAL-AUDIT.md) is a generated artifact nothing regenerates, and two chromes make two documents that lie | Designed | P3 | `tests/browser/ux-visual-audit.mjs` exists and **no file under `.github/workflows/` references it**, so the checked-in document drifts silently — its `window.prompt` findings were fixed by the row-height/column-width change and the document was never rewritten. Its two live findings are real and narrow: `webapp/editor.dialogs.js:1832` and `:1834` build the hyperlink dialog's only two buttons without the `oc-btn` class, so they render 21 px against the audit's own 24 px floor — the only such site in `webapp/`, since `.oc-btn.primary` (`webapp/editor.css:616`) is the only `primary` rule and `.oc-btn` (`:582-584`) is what supplies the padding. Either wire the harness into CI so a drifted checked-in copy fails the build, or delete the checked-in copy and make it a CI-only report. **Per `CLAUDE.md`, a document stating a contract the code does not keep is a row, not a prose fix** | The two-token `oc-btn` fix measured against the 24 px floor; and the harness either running in `browser-smoke` with a drift check, or the checked-in copy gone |
| `<assign>` | The ADR-status gate silently skips every three-segment tracker id | Designed | P3 | `docs/91` §9 R-15 records that the gate filters rows with `^[A-Z]{2,6}-\d+$`, a two-segment id, so a `Done` row citing an ADR left `Proposed` would pass unnoticed. **Every row above is three-segment**, as is every `UX-RIB` row. The gate moves, not the id scheme | The gate's own self-test (`tools/check-gate-selftest.py`) extended with a three-segment row that must be caught; filed **before** any `UX-GS` or `UX-RIB` row closes |

---

## 17. Questions that are the product owner's

**1. The web mount's default is the one real coin-flip. Confirm `toolbar`.**
§12.7's case is revertibility: `docs/91` R-12's whole mitigation is that every
slice up to the flip is revertible by changing one default, which only holds if
the shipped web default is the incumbent. The honest counter is that Microsoft
ships Simplified on the web and `docs/91` §4.1 calls the web this product's
primary mount. If the answer is "ribbon on the web too", say so now, because it
converts a revertible programme into a flip and the number to agree in advance
is the content share, not the direction. Desktop is **not** a question: you
named it, and it is the ribbon.

**2. "Ribbon" and "Toolbar" as the chooser's words — confirm.** Not "Excel" and
"Google Sheets": both are third-party trademarks, and
`tests/browser/editor.branding.spec.mjs:57` asserts no region of the chrome
names a product. Not "Classic"/"Simplified", which the ribbon's own internal
toggle already owns. If you want different words, they need to be legally safe
and not already spent — and the one-line descriptions under them ("Tabbed ribbon
with grouped commands" / "Menu bar with a single toolbar row") are where the
recognition actually happens.

**3. Is `View ▸ Layout` the right home, or should the chooser also appear on
first run?** The design says View, following `UX-CHR-01` and LibreOffice, and
refuses a second control in Settings because two controls for one setting drift.
What it does *not* do is offer the choice to a new user who does not know there
is one. A one-time first-run card is a legitimate alternative and it is a
different slice; it is not in §15.

**4. Ctrl+F1 for compact controls in both chromes, diverging from Sheets.**
Sheets binds Ctrl+Shift+F, which is Format Cells here
(`webapp/editor.core.js:8470`), moved off Find deliberately with nine lines of
comment arguing against exactly this kind of rebinding. So one verb gets one
chord across both chromes, which is better than either chrome matching its own
source. Confirm, and it goes in the release note.

**5. Where does the collaboration surface stop?** §4 draws presence, Share, the
save state, comment history and version history. It does **not** draw Meet, an
activity dashboard, or an assistant panel, because there is nothing behind any
of them. If the answer is "collaboration is the differentiator, draw more", the
next thing to build is not chrome — it is the `docs/89` cell edit-history card,
which is the honest substitute for the suggestion mode Sheets does not have and
which would make us better than the product we are imitating rather than equal
to it.

**6. Does the desktop shell keep this chrome available at all?** §12.3 says yes
— `chrome: "native" × layout: "toolbar"` stays representable, and the chooser
appears in the OS menu because `View` is drawn from `menuModel()`. The
alternative is pinning the desktop to the ribbon outright, which is simpler and
which your sentence could be read as asking for. It changes one line and it
removes a user's choice, so it should be a decision rather than an inference.

**7. Where does this sit against the open backlog?** Twelve slices is a
programme, not a task, and it lands on top of thirteen ribbon slices that share
its first three. By the tracker's own severity rubric chrome work is P2 —
nothing here reaches the file — and `DEP-08` is the only open P1. Two chrome
programmes running at once should be scheduled as such or explicitly promoted;
what should not happen is both being worked opportunistically, because §15's
wave rule forbids most of the pairings and a merge conflict between two agents
costs more than the parallelism saved.

---

## 18. Reproducing this

**What was read**, in full or in the cited region, in this round:

- `webapp/editor.html` — the header (`:46-104`), the menu bar (`:166-200`), the
  toolbar (`:240-493`), the formula bar (`:495-526`), the grid block
  (`:555-583`), the bottom bar (`:586-657`)
- `webapp/editor.css` — the token blocks (`:70-122`, `:128-151`, `:172-215`),
  the region-hiding rules (`:250-270`), the region heights (`:303`, `:356`,
  `:415`, `:994`, `:1037`), the app-bar type (`:324-332`), the toolbar states
  (`:763-772`), `.tb-icon` (`:779`), the popmenu block (`:850-890`), the
  presence block (`:450-477`), the menu bar (`:530-546`), the formula bar
  (`:990-1032`), the bottom bar and tabs (`:1037-1056`), `.work-area` /
  `.grid-wrap` / `.side-panel` (`:1297-1305`), the reduced-motion clamp
  (`:1505`), `prefers-contrast` (`:1886`), the native-chrome block (`:2052-2058`)
- `webapp/editor.core.js` — the capability presets (`:945-995`),
  `CHROME_REGIONS` (`:850`), `placeNativeChrome` (`:1310`),
  `CAPABILITY_COMMANDS` (`:1336-1358`), `CHROME_ONLY` (`:1377`),
  `READ_ONLY_SAFE` (`:4988-5010`), `buildChartPanel` (`:4417-4490`),
  `openPanel`/`closePanel` (`:5145-5175`, `:5463`), `cellAuthor` and
  `buildHistoryPanel` (`:5203-5215`), `buildNotePanel` (`:5534-5552`),
  `initTooltips` (`:5724`), `renderTabs`'s add/all buttons (`:7040-7072`), the
  key map (`:8418-8472`, `:8632-8654`), `anchorMenu` (`:8806`), the doc-identity
  publisher (`:9170-9200`), the toolbar wiring and roving tabindex
  (`:9426-9476`), `MENUS` (`:9611-9950`), `buildMenuBar` (`:10081-10234`), the
  panel Escape handler (`:10240-10252`), `wireSettings` (`:10268`), the boot
  sweep (`:11015-11024`)
- `webapp/editor.selection.js` — `commandId` (`:1015`), `listCommands` (`:1040`),
  `menuModel` (`:1065-1136`), `runCommand` (`:1146`), `applyCommandRules`
  (`:1166-1286`), `autoSum` (`:1320`), `doUndo` (`:1383`)
- `webapp/editor.dialogs.js` — the dialog and panel inventory (`:89`, `:141`,
  `:590`, `:641`, `:1060`, `:1195`, `:1332`, `:1528-1546`, `:1788`, `:1828-1838`,
  `:1867`, `:2075-2094`, `:2159`, `:2244`, `:2295`, `:2451`, `:2634`, `:2792`,
  `:2879`)
- `webapp/editor.sheets.js` (`:120-215`, `:283`), `webapp/editor.presence.js`
  (`:255-268`), `webapp/editor.paint.js` (`:895-925`),
  `webapp/editor.geometry.js` (`:226-240`), `webapp/editor.i18n.js` (`:83-108`,
  `:198-208`), `webapp/editor.drafts.js` (`:513-524`)
- `webapp/embed.js` (`:95-102`, `:258-266`), `webapp/embed.d.ts` (`:70-85`)
- `desktop/src/main.rs` (`:925-930`), `desktop/tauri.conf.json` (`:15-22`)
- the browser suite's chrome specs by name and line, as cited throughout
- `docs/91` in full, `docs/92` §5 in full, `docs/88` for its measurements, and
  `docs/69`, `docs/82`, `docs/83` for the claims attributed to them

**What was NOT run.** No build. No `wasm-pack`, no `webapp/serve.py`, no
Playwright, no browser of any kind. **No file in `webapp/`, `crates/`,
`server/`, `desktop/` or `sdk/` was modified. `docs/14` and `docs/08` were not
opened for writing.** The only file this round wrote is this note.

**Consequently every width figure in §6.4 is arithmetic over §8.1's metrics, not
an observation.** The 1305 px budget and every slack figure in the ladder are
computed. §15's slice 08 is how they become measurements, and if the arithmetic
is wrong the first collapse step moves — **the order does not, because the order
is authored and not derived from the number.**

**To reproduce the region-height table in §3.1**, without a browser:

```
grep -n -A3 '^\.app-header\|^\.menubar\|^\.toolbar\|^\.formula-bar\|^\.bottom-bar' webapp/editor.css
```

Each declares its own `height` and its own `border-bottom` / `border-top`; the
figures are those two numbers added, and nothing else contributes, because every
band is `flex: 0 0 auto` in one column (`webapp/editor.css:227-233`).

**To reproduce the caret finding in §1.8:**

```
grep -n 'aria-haspopup' webapp/editor.html   # ten .tb-btn matches
grep -n 'aria-haspopup' webapp/editor.css    # nothing
```

**To reproduce the command count — and it has never been done.** Boot the editor
and read `window.opencalcEditor.listCommands().length`. `webapp/docs.html:353`
advertises 189 and `docs/91` §3.12 asserts 188; both are strings in documents,
and `file.download`'s format rows are generated at runtime from
`writable_extensions()` (`crates/casual-calc-wasm/src/io.rs:267`), so the true
number is build-dependent. **Measure it before it enters a gate or a document.**
