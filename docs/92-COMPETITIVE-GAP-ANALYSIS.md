# 92 — Competitive gap analysis: what are we missing?

**Produced 2026-09-09 against `HEAD` = `cd59836`.** Against **Microsoft Excel
(Microsoft 365 Current Channel), Google Sheets (web) and ONLYOFFICE Spreadsheet
Editor (Docs 9.4)** — the three the product owner named.

> **Read this before briefing work from this document.**
>
> [12](12-COMPETITIVE-ANALYSIS.md) opens by recording that it was caught wrong
> six times in one week, and that two of those six survived because *a
> `file:line` was quoted and never opened*. That failure mode was live again in
> this round: the surface inventory handed to the agents that produced this
> document contained at least four wrong claims about OpenCalc, three of them
> pessimistic. Each is corrected in §2.4 with the line that settles it.
>
> The operating rule is unchanged and is the reason this document exists in the
> shape it does: **a measurement without a date is a claim about a tree that has
> moved, and a citation is only evidence if somebody reads the line.** Every
> OpenCalc `file:line` below was opened during this pass. Every competitor claim
> carries a source and the date it was fetched. Where neither was possible, the
> row says `unverified` and means it.

---

## 1. What this is, and its relationship to `docs/12`

`docs/12` is dated **2026-08-29, audited 2026-08-30**. It compares **five**
products (Excel, Sheets, ONLYOFFICE, LibreOffice Calc, Univer) **task by task**,
and its organising question is *"how good is each thing we have?"*

This document is dated **2026-09-09**, compares **three** products, and asks one
question: **what are we missing?** It is a gap ledger, not a task audit. Where
`docs/12` says "present but worse", this says nothing at all — that is `docs/12`'s
job and it still does it. Where `docs/12` says "absent", this re-derives the
claim from the code, prices it, and says whether the engine already has it.

Eleven days separate them, and in this repository eleven days is a long time.

### 1.1 What this supersedes

These `docs/12` sections state things that are **no longer true or were never
true**. Treat this document as the current answer and do not brief from them
until they are corrected in place (`DOC-051`, `docs/14:94`, already names four of
these):

| `docs/12` section | Why it is superseded |
| --- | --- |
| §3.15, §8 item 14 (structured references) | Says `[@Column]` → `#VALUE!` and files it `[unverified]` from `docs/73` #10. Half right. `Sales[Amount]` **works and is tested**; three *current-row* forms fail, one of them silently wrong. §4.1 gap C-1 below. |
| §3.17, §8 item 5, §10.1(d) (PDF) | "Absent: PDF export … `grep -ri pdf` returns only comments." PDF export ships and is reachable — `webapp/editor.core.js:9701`, `IO-14` Done. Already named wrong by `DOC-051`. |
| §3.19, §8 item 2, §10.1(c) (save) | "There is no save … no autosave, no version history." `webapp/editor.drafts.js` and `webapp/editor.versions.js` both ship; `SAVE-03`, `SAVE-08`, `SAVE-13`, `HIST-01/02/03` all Done. The residual is the *save target* in a browser tab. |
| §3.21, §8 item 11 (comments) | "No replies." Threaded replies exist end to end — `crates/casual-calc-wasm/src/objects.rs:254` `session_reply_comment`, model field at `crates/casual-calc-model/src/sheet.rs:826-830`. This was **false when written**: the code landed 2026-08-22, seven days before the measurement. Seventh instance of the class the preamble warns about. |
| §3.8 (data validation) | The section's only "Worse" item — the dialog not showing the existing rule — is closed (`DV-04`). `loadExisting()` at `webapp/editor.dialogs.js:1676`. `docs/47` is also stale here and needs regenerating. |
| §3.14, §4.3, §10.1(j) (`window.prompt`) | There is no `window.prompt` left in `webapp/`. Closed by `UX-DLG-02`; `webapp/editor.dialogs.js:2621` records the removal. |
| §4.1 / §4.2 counts | "Help ▸ Keyboard shortcuts lists only eight rows" — it lists **fifteen** (`webapp/editor.core.js:9573-9587`; `TAURI-012` added the file chords). The point survives, the number does not. |
| §3.22, §8 item 3 (sharing) | Share is live in `standalone` and `desktop` (`webapp/editor.core.js:955`, `:958`). The comment one line above the menu item at `:9668` still says the opposite. |
| §5 item 6 (SDK) | Caveat reads "0.0.0 and no type declarations shipped (`SDK-009`)". All three packages are `0.0.1` and all three declare types. `CLAUDE.md`'s current-state paragraph carries the same stale version. |
| §8 items 1, 4, 10, 12, 15, 16 | Already struck in place: the row-74,567 overflow (`PERF-D-03`, fixed at `crates/casual-calc-wasm/src/axis.rs:193-201`), formula CF (`CF-01/02/03`), `.xlsm`/`.ods` (`IO-07/08`), the chord tally, zoom (`UX-CHROME-05`), the a11y mirror (`A11Y-01`). |

### 1.2 What this corroborates

Re-derived independently this pass and found still true. `docs/12` remains the
better account of *how* bad each is; this document adds price and seam:

- §3.23 — no macros, no scripting, no add-ins, no external data. Unchanged.
- §3.24 — no Goal Seek, Solver, Scenario Manager, Data Table, Subtotal.
- §3.25 — no picture insert, no shapes, no text boxes, no form controls.
- §3.5 — the three-key sort cap. Still `[0, 1, 2].forEach(addKeyRow)` at
  `webapp/editor.dialogs.js:641`.
- §3.12 — the Name Manager still manages nothing. Two controls per name,
  `webapp/editor.dialogs.js:2257-2283`.
- §4.2 — no command palette. Still the cheapest discoverability win available,
  and §5 below shows why it is cheaper than it looks.
- §10.2 — all seven "genuinely unsolved" items stand, with one status change
  (`UX-DESK-01` is Done, so item 5's citation is stale) and one addition
  (`IO-12`, the compatibility-outcome vocabulary, is the answer to item 7).

### 1.3 What this leaves standing untouched

`docs/12` §3.1-§3.4, §3.6, §3.9-§3.11, §3.13, §3.16, §3.18, §3.20, §4.3-§4.7,
§7 (the divergence ledger) and §9 are not re-derived here. They are task-quality
findings, not gaps, and re-deriving them would have cost the verification budget
this document spent on the fifty-eight gaps instead. **They are also eleven days
old.** Anything in them that is going to be briefed should be re-opened first —
five of the eleven stale items in §1.1 were found by doing exactly that.

`docs/12`'s coverage of **LibreOffice Calc and Univer** is not superseded,
corroborated or replaced. This round was scoped to three products by the owner.

---

## 2. How this was produced, and how far to trust it

### 2.1 What was done

Four read-only agents, one per gap bucket, each given: a code map, one bucket,
the three competitors' surface inventories (produced by four earlier agents from
published documentation on 2026-09-09), and two rules — cite `file:line` for
every OpenCalc claim, cite a source **and a date** for every competitor claim.
Their reports were then checked: every OpenCalc line quoted in a `P0` or `P1` row
below was opened by hand during the write-up, and the ones that changed the
answer are listed in §2.3.

Two agents ran code rather than only reading it. The date-parsing finding (gap
X-1) and the structured-reference finding (gap C-1) are both the product of a
throwaway crate built against the repository crates by path and run on
2026-09-09; the probe outputs are quoted verbatim in those rows. Nothing was
written into the repository to get them.

Competitor claims come from `support.microsoft.com`, `learn.microsoft.com`,
`support.google.com`, `developers.google.com`, `helpcenter.onlyoffice.com`,
`api.onlyoffice.com`, the ONLYOFFICE blog, and the `ONLYOFFICE/web-apps` and
`ONLYOFFICE/sdkjs` git trees read through the GitHub API. Every one is dated
2026-09-09 unless the page itself carries a publication date. The full list is
§11.

### 2.2 What was NOT done, and what that costs

**Nothing here was reproduced in a running editor.** No browser was driven, no
build was made, `webapp/pkg` does not exist in this checkout. Reachability is
traced statically — UI entry point → wasm export → model mutation — which is
strong evidence and is not the standard `CLAUDE.md` sets.

This matters more here than it would elsewhere. `docs/73` records that
`UX-GRID-02` was found *by using the editor for ten minutes*, and states the
standing assumption plainly: **driving it finds things reading it does not.**
Expect running this list to reorder it. The specific places where a browser
would change an answer are named in §10.

Three further limits, stated so nobody has to discover them:

1. **No competitor was exercised either.** Every Excel, Sheets and ONLYOFFICE
   claim is documentation, not a test run. Where the documentation is silent —
   ONLYOFFICE's iterative-calculation setting, filter-by-colour, Subtotal and
   Flash Fill; Sheets' RTL sheet direction; Excel's exact icon-set count — the
   cell says `unverified` rather than guessing. A wrong claim about Excel would
   discredit everything around it.
2. **Effort is a judgement, not an estimate.** `small` means one function or one
   dialog; `medium` means one subsystem; `large` means a project; `very large`
   means a decision first. Nothing here was scheduled or spiked.
3. **Severity follows the tracker's rule** (`docs/14`, "How a row works"): by
   what reaches the *file*, not by what shows on screen. That is why a menu
   routing gap is `P3` and a typed date that becomes text is `P0`.

### 2.3 Numbers this pass re-derived

`docs/12` §1's counts table is eleven days old and three of its eight figures
have moved again. Re-measured 2026-09-09:

| Quantity | `docs/12`, 2026-08-30 | 2026-09-09 | Where |
| --- | --- | --- | --- |
| Engine functions | 429 | **429** | `crates/casual-calc-eval/src/functions/mod.rs:59`, parsed; gated by two bijection tests, so this one is safe to quote |
| WebAssembly bindings | 277 / 241 `session_*` | **294** / **259** | `grep -c '#\[wasm_bindgen\]' crates/casual-calc-wasm/src/*.rs` |
| Crates | 16 | **16** | `ls crates/` |
| Editor JavaScript | ~19,900 / 18 modules | **23,327 lines / 20 modules** | `wc -l webapp/*.js` |
| Editor commands | 197 | **not re-measured** | needs a driven browser |
| Toolbar controls | 109 | **not re-measured** | needs a driven browser |
| WASM payload | 7,889,202 bytes | **not re-measurable** | `webapp/pkg` absent in this checkout; a PDF writer and embedded fonts have landed since |
| Cold boot | 334 ms | **not re-measured** | needs a driven browser |

**Do not requote the last four.** They are the ones that need a browser and they
are the ones most likely to have moved.

### 2.4 Corrections carried through from this round

Beyond the `docs/12` supersessions in §1.1, this pass contradicts the surface
inventory it was built on and three other documents in the tree. Each is settled
by a line that was opened.

| Claim, and where it is made | The correction |
| --- | --- |
| **Surface inventory**: "No per-cell accessibility tree … the honest ceiling of a canvas renderer", citing `webapp/editor.html:557` | Wrong, and it under-reports the product's strongest accessibility claim. `webapp/editor.core.js:6728/6733` set `role="columnheader"`, `:6746` `role="rowheader"`, `:6753` `role="gridcell"`, `:6726/:6744` `aria-rowindex` — with **absolute** indices against the sheet's declared counts, so "row 4,201 of 1,048,576" stays true with 40 rows in the DOM (`:6571-6573`). The cited line is evidence about the renderer, not about the mirror. |
| **Surface inventory**: "Multi-range (non-contiguous) selection — ABSENT as far as I could trace" (marked `unverified`) | Present. `webapp/editor.core.js:3662-3663` banks a range on Ctrl/Cmd+click, `:2309` tests membership against the bank, `:2718` tints it, `:7215-7229` records that bold, Delete and copy all read it. `docs/12` §3.3 already had this right; `docs/47` marks both rows ✅. |
| **Surface inventory**: "Shrink-to-fit … round-trips through the file but does not render" | It renders. `crates/casual-calc-wasm/src/axis.rs:815-817` emits `"shrink":1`; `webapp/editor.core.js:3014-3020` scales the font down until the text fits. Only the *control* is missing. `docs/53` FC-14 🟡 is stale on this half and should be split. |
| **Surface inventory**: describes `casual-calc-sdk` as the engine's API without qualification | All sixteen crates carry `publish = false`, `casual-calc-sdk` included. There is no consumable Rust binding and no server-side generation story — a materially different position from "the SDK exists". §4.3 gap P-7. |
| **Surface inventory**: "PDF export … through the same paginator the PDF path uses" for print | The two paths do not share pagination. `crates/casual-calc-wasm/src/objects.rs:1771-1793` states the engine does **not** paginate for the HTML path and the browser does; the PDF path uses `casual_calc_layout::print::Plan`. They produce different page breaks *and* different content. |
| **`docs/80` and `docs/12` §5**: "one display list, three executors — a chart cannot look one way on screen and another in an export" | True for charts, **not for cells**. `PaintItem::Text` (`crates/casual-calc-layout/src/display.rs:221-246`) carries no rotation, wrap, vertical alignment, underline, strikethrough or indent; the canvas draws all six. PNG and PDF paint a materially different cell from the screen, and `session_export_pdf_loss` does not report it. §4.2 gap R-4. |
| **`SIGN-01`** (`docs/14:122`): "silently invalidated" | Right, and incomplete in a way that changes the remedy. `crates/casual-calc-import/src/lib.rs:269-274` retains any part reachable from the package root, and retained parts are re-emitted verbatim (`crates/casual-calc-model/src/workbook.rs:31`) — so a signature is not dropped, it is written back **stale over changed content**. A present-but-broken signature reads as tampering; an absent one reads as unsigned. |
| **`docs/51`**: lists `calcPr` iterative calculation among unimplemented constructs | Implemented and tested. `crates/casual-calc-model/src/workbook.rs:252` `iteration()`, Excel's own 100/0.001 defaults, `crates/casual-calc-eval/src/tests.rs:3943-4010`. The gap is that no wasm binding sets it — `grep -rn iterat crates/casual-calc-wasm/src/` returns nothing. A document under-reporting the product, which is the rarer direction. |
| **`crates/casual-calc-formula/src/lib.rs:14`** | "Deferred: R1C1, full row/column ranges (`A:A`), 3-D refs, structured (table) references, and union/intersection." Whole-column ranges **and** structured references both parse and evaluate (`crates/casual-calc-eval/src/ranges.rs`; `crates/casual-calc-eval/src/tests.rs:1332`, `:2298-2360`). Genuinely deferred: R1C1, 3-D refs, array constants, intersection. A crate-level doc comment is exactly the citation a reader trusts without opening. |
| **`crates/casual-calc-sdk/src/lib.rs:2575`** and `tests.rs:1078` | Both read "`SUBTOTAL`'s 101–111 codes and `AGGREGATE` skip…" as a statement about current behaviour. `AGGREGATE` is not among the 429 catalogued functions. Not a defect — the prose is describing the visibility contract — but it will be quoted as evidence the function exists. |
| **`tools/feature-audit/inventory.py`** | `docs/53` no longer regenerates. `inventory.py:20-21` reads `webapp/editor.js` (now a 17-line re-export shim) and regexes `pub fn session_*` out of `crates/casual-calc-wasm/src/lib.rs` (now a 4-export module root after the 16-module split). Running it prints `FNS=0/MUT=0/UNDO=0` for all 29 areas. `docs/14`'s claim that `docs/53` "cannot drift in the way these did" no longer holds. |
| **`benchmarks/baselines/dev-reference.json`** | The benchmark tool defines at least seven case ids (`model-snapshot-roundtrip-10k`, `package-open-small`, `display-list-frame-serialise`, `render-frame-1600x900`, `eval-incremental-edit-scaling`, `eval-kept-graph-edit-scaling`, `eval-kept-graph-range-edit-scaling`). The committed baseline holds **two**, both micro-cases, and **no eval case at all**. §4.4 gap X-7. |

---

## 3. The answer in one page

**Fifty-eight gaps.** Five are `P0`. **Sixteen would end an Excel user's
evaluation** — see §7 for the ranked list.

If you read nothing else:

**1. A typed date becomes text.** `12/31/2026`, `31/12/2026`, `31-Dec-2026` and
`1,234.5` all store as strings. Only strict ISO `YYYY-MM-DD` parses
(`crates/casual-calc-sdk/src/lib.rs:1160`, `crates/casual-calc-io/src/lib.rs:249-253`;
probe run 2026-09-09). The column will not sort, `MAX` over it is wrong, and the
`.xlsx` handed back to Excel carries text where Excel wrote dates. The only cue
is left alignment. This is not a locale gap — a US Excel user typing `12/31/2026`
hits it in the first minute.

**2. An edited save destroys worksheet `<extLst>`.** Sparklines, x14 icon-set
rules, x14 data validations and slicer references are read nowhere and written
nowhere — one `extLst` match in the entire import/export surface and it is inside
a test allowlist (`crates/casual-calc-import/src/tests.rs:614`). An unedited file
saves byte-identically, so the loss fires **only after the first edit**, which is
the worst possible timing for noticing it. Nothing counts it. This is a straight
breach of the repository's own no-silent-loss rule, in the format the repository
is best at.

**3. Two workbooks with the same filename share one version history, and the
second one deletes the first's.** `docKey()` is the display name alone
(`webapp/editor.versions.js:77`), `store.put({ key: doc, versions: manifest })`
replaces the whole row (`:136-138`), and a prefix sweep then deletes every byte
row the new manifest does not name (`:141-150`). `forgetVersions()` is called
from exactly one place — File ▸ New — so Open never clears the bucket. Renaming
has the mirror effect. The sibling store in the same design got this right:
drafts key by rotating slot id (`webapp/editor.drafts.js:175`).

**4. A calculated column in an Excel table opens as errors, and one form gives a
plausible wrong number.** `=Sales[@Amount]*1.2` → `#VALUE!`,
`=SUM(Sales[[#Data],[Amount]])` → `#REF!`, and `=SUM(Sales[#This Row])` silently
returns the whole table's total. The cause is one `match` arm
(`crates/casual-calc-eval/src/eval.rs:215-233`). `Sales[Amount]` works and is
tested — so both `docs/12` §3.15 and `docs/73` #10 are wrong about which half is
broken.

**5. Charts already do stacked, combo, secondary axis and data labels — the
panel has no control for any of them.** `crates/casual-calc-wasm/src/objects.rs:98/:130/:133`
accept `grouping`, `secondary_axis`, `data_labels`; `webapp/editor.core.js:4404`
offers seven kinds and no subtypes. `docs/12` §8 item 6 still prices this as a
chart project. It is four `<select>`s.

**6. Nothing ships translated, and nothing is right-to-left.** One built-in
locale (`webapp/editor.i18n.js:83-84`); `direction: ltr` hard-set on the shadow
root (`webapp/editor.css:59`) with 53 physical `left`/`right` declarations
beside it; no IME composition path, so a CJK user cannot begin typing into a
selected cell at all (`webapp/editor.core.js:8664`). ONLYOFFICE ships 46
catalogues for the same editor and 118 for its mobile one.

**7. There is no automation of any kind, and that is the largest categorical
loss.** No recorder, no scripting, no custom-function registration, no add-in
host. All three competitors have two or more answers each. `docs/12` §10.2
correctly files this as a decision rather than a schedule, and it still is.

**The cheapest round available.** Nine gaps are engine-capable and
editor-unreachable (§5). Together they are days, not weeks, and they close one
`P0`, four `P1`s and four lower rows. That is a better first round than starting
any single item above except the `P0`s.

---

## 4. The gap tables

Four buckets, fifty-eight gaps, hardest-hitting first within each. Columns are
verdicts; the evidence for OpenCalc is the `file:line` in the "OpenCalc today"
cell, and the competitor sources are consolidated in §11 with dates.

`Sev` follows `docs/14`'s rule — by what reaches the file. `Conf` is `V` where a
line was opened or code was run this pass, `D` where the claim rests on a
document in the tree, `U` where it could not be settled.

### 4.1 Calculation and data — 14 gaps

| Gap, as a task | Excel | Sheets | ONLYOFFICE | OpenCalc today | Sev | Effort | Conf |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **C-1** Write a table formula that refers to its own row (`[@Amount]`, `[#This Row]`, `[[#Data],[Col]]`) | yes | yes | yes | `#VALUE!`, `#REF!`, and — for `[#This Row]` — the **whole table's total** where Excel gives the row's value. One `match` arm: `crates/casual-calc-eval/src/eval.rs:215-221` falls to `_ => (first_data, last_data)`, `:224` sends `#This Row` to the full column span, `:227-230` returns `None` for `@Amount`. `Sales[Amount]` works (`tests.rs:1332`) | **P0** | small | V |
| **C-2** Ask what input produces a target result (Goal Seek, Solver, Scenario Manager, Data Table) | yes, all five incl. three Solver engines | Goal Seek as a Google-published add-on only | Goal Seek (8.0) + Simplex-LP-only Solver (9.3); no Scenario Manager, no Data Table | Nothing. `grep -rni "goal seek\|solver\|scenario manager\|data table"` over `webapp/ crates/` returns only an unrelated `resolver` and one comment in `functions/financial.rs:255`. Data menu complete at `webapp/editor.core.js:9885-9925` | P1 | medium | V |
| **C-3** Express a pivot value as a share; add a computed measure; group dates by day or number by band | yes — 15 Show Values As, calculated fields and items, full grouping, slicers, timelines | shares and calculated fields yes; slicers yes; grouping undocumented | calculated items, grouping and slicer dialogs all present in the shipped tree | Vocabulary only, and the source says so twice. `crates/casual-calc-model/src/pivot.rs:145` "Nothing on `PivotValueField` carries this yet"; `:385` the same for calculated fields; `:273` "the first release honours Years, Quarters and Months". No slicer or timeline type anywhere | P1 | large | V |
| **C-4** Pull data in from a database, a web page or another workbook, and refresh it | yes — Power Query + Power Pivot | yes — IMPORT* family with hourly refresh, Connected Sheets | no Power Query; external links resolved through the host, async custom functions since 9.0 | Nothing. No connector functions among the 429; no connection surface among the 294 bindings; no network layer by design (`AGENTS.md`), collab server explicitly stateless (`server/casual-calc-collab-server/src/lib.rs:1-7`) | P2 | very large | V |
| **C-5** Split a CSV column with quoted fields without destroying the columns beside it | yes — qualifier, fixed width, per-column type, destination, overwrite prompt | partial | yes, multi-separator since 9.3 | Nine lines of JavaScript, no engine binding: `const parts = text.split(delim)` at `webapp/editor.dialogs.js:2210`, then `session_set_cell` at `:2212-2214` with no bounds and no occupancy check. `grep -rn text_to_columns crates/` returns nothing | P1 | medium | V |
| **C-6** Filter by colour, by relative date period, by top-10; use a criteria range | yes, all | colour and condition yes | AutoFilter; colour/advanced unconfirmed | Values plus one or two comparisons over six operators (`crates/casual-calc-model/src/sheet.rs:459-476`). `<top10>`, `<dynamicFilter>`, `<colorFilter>`, `<iconFilter>`, `<dateGroupItem>` are carried and **not evaluated** — `sheet.rs:429-443`, "The rows it would hide are left visible" | P2 | medium | V |
| **C-7** Reshape an array or split text in one function (VSTACK, TAKE, TEXTSPLIT, REGEX*, AGGREGATE) | yes, all | most | dynamic arrays + regex since 9.3; TAKE/stacking still open requests | 429 catalogued, parsed programmatically. Absent: 25 named functions incl. the whole 2022 shaping family and 2024 GROUPBY/PIVOTBY set. Present and ahead: full LAMBDA family with real `#SPILL!` blocking | P2 | medium | V |
| **C-8** Sort on more than three columns, or by colour, or by a custom list | yes — 64 levels, colour, icon, custom list, left-to-right | colour yes, no documented cap | yes, with a sort-options dialog and API | `[0, 1, 2].forEach(addKeyRow)` — `webapp/editor.dialogs.js:641`. Binding takes `key_cols: Vec<u32>, ascending: Vec<u8>` only (`crates/casual-calc-wasm/src/structural.rs:421-422`) | P2 | small | V |
| **C-9** Rename a defined name, change what it refers to, or scope it to one sheet | yes, all | rename and range yes; no sheet scope | yes — name manager, edit and paste dialogs | Two controls per name: go-to (`webapp/editor.dialogs.js:2257-2269`) and delete (`:2270-2283`). Four bindings and none is an update. `session_define_name` hardcodes `sheet: None` at `crates/casual-calc-wasm/src/objects.rs:1403`, so a sheet-scoped name cannot be created | P2 | small | V |
| **C-10** Say which columns make a row a duplicate | yes — per-column checklist | yes | yes, with a removed/remaining report | The binding compares the whole rectangle: `session_remove_duplicates(sheet, first_row, c0, r1, c1)` — `crates/casual-calc-wasm/src/clipboard.rs:1295-1301`, no column mask | P2 | small | V |
| **C-11** Build a model with a deliberate circular reference | yes | yes | unverified | **Engine has it**; nothing can turn it on. `crates/casual-calc-model/src/workbook.rs:252` + `crates/casual-calc-eval/src/tests.rs:3943-4010`. `grep -rn iterat crates/casual-calc-wasm/src/` returns nothing; Tools ▸ Calculation offers Automatic / Manual / Calculate now only | P2 | small | V |
| **C-12** Turn a sorted list into a subtotalled, outlined report in one action | yes — Subtotal + Auto Outline | no | grouping yes; Subtotal command unconfirmed | Manual grouping is complete and good (`webapp/editor.core.js:9908-9920` over `crates/casual-calc-wasm/src/axis.rs:1048-1179`); the command that *generates* it is absent. `SUBTOTAL` the function exists and respects filtered rows | P3 | small | V |
| **C-13** Have the machine finish a column from examples (Flash Fill); consolidate ranges by label | yes to both | Smart Fill, and it writes a *formula* rather than literals | neither found | Neither exists. `grep -rni "flash ?fill\|smart fill\|consolidate" webapp/ crates/` returns nothing. Ctrl+E is one of only two Excel chords in the parity suite that do nothing | P3 | large | V |
| **C-14** Know whether a real 200k-formula model will keep up | yes — 64-core calc | partial — a published 10M-cell ceiling | unverified | Architecture right, evidence absent. Seven benchmark case ids defined; **two** baselined, both micro, **no eval case**. See also X-6 (the 424 ms main-thread block) | P3 | large | V |

**Notes.** C-1 and C-5 are the two rows where the user gets a wrong result rather
than a missing one, and C-1 is the only one of the fifty-eight that produces a
plausible wrong *number* without a marker. C-6's `Unevaluated` design is honest
and well argued in its own comment — the residual cost is that `SUBTOTAL` and the
status-bar aggregates read differently from Excel with nothing on screen saying
why. C-11 is the clearest engine-capable row in the bucket (§5).

### 4.2 Presentation and output — 15 gaps

| Gap, as a task | Excel | Sheets | ONLYOFFICE | OpenCalc today | Sev | Effort | Conf |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **R-1** Open a workbook with sparklines, edit one cell, save, and still have them | yes | no sparkline object, but SPARKLINE() survives its own round trip | yes — create dialog and a Sparkline ribbon tab | **Silently destroyed.** `extLst` is read nowhere and written nowhere — one match in `crates/casual-calc-import/src/tests.rs:614`, a test allowlist. `Sheet::carried` (`crates/casual-calc-model/src/sheet.rs:246-253`) does not include it. Unedited files save byte-identically, so the loss fires only after the first edit. No `report.record` catch-all for an unrecognised worksheet element | **P0** | medium | V |
| **R-2** Put a picture on a sheet; move, resize or delete one that arrived in a file | yes, incl. image-in-cell and IMAGE() | yes, both modes plus IMAGE() | yes | Read-only. Complete Insert menu at `webapp/editor.core.js:9800-9817` has no Image item; the only two image bindings are `session_images` (`crates/casual-calc-wasm/src/view.rs:416`) and `session_image_data` (`objects.rs:1505`), both read. Painted at `webapp/editor.paint.js:85-106`; no hit-test, no handles. PDF refuses to draw them (`crates/casual-calc-render/src/pdf.rs:883-889`) | P1 | large | V |
| **R-3** Make a stacked, 100%-stacked or combo chart; add a secondary axis or data labels | yes — 17 types, ~50 subtypes | yes | yes | **Engine has all four**; panel offers none. `CHART_KINDS` at `webapp/editor.core.js:4404-4407` is seven kinds, no subtypes; `grep -n 'grouping\|secondary\|dataLabels' webapp/editor.core.js` returns nothing. Wire accepts them at `crates/casual-calc-wasm/src/objects.rs:98/:130/:133` | P1 | small | V |
| **R-4** Print or export a page that looks like the sheet | yes — WYSIWYG | yes | yes | Two paths that disagree, and the losses are half-reported. Ctrl+P drops charts, images and **all** conditional formatting (`crates/casual-calc-wasm/src/objects.rs:1820-1823`), wraps nothing and pins vertical alignment to bottom. PDF keeps charts and CF, drops pictures, **and flattens rotation, wrap, vertical alignment, underline, strikethrough and indent** because `PaintItem::Text` has no field for them (`crates/casual-calc-layout/src/display.rs:221-246`). `session_export_pdf_loss` names only the pictures | P1 | large | V |
| **R-5** Draw an arrow, a callout or a text box on a sheet | yes — full shape gallery, SmartArt, icons | yes — Insert ▸ Drawing | yes — autoshapes, SmartArt, TextArt, Draw tab | No Shape or TextBox type in any of the sixteen model modules. `PaintItem` has ten variants and no shape primitive above the chart plotter's own Polyline/Polygon/Wedge | P1 | very large | V |
| **R-6** See where the pages will fall before printing | yes — Page Layout and Page Break Preview | yes — live preview, drag breaks | yes — navigable preview | No preview of any kind: `grep -rn 'page.break.preview\|pageBreakPreview\|Page Layout\|printPreview' webapp/*.js` returns nothing. **Manual** breaks are drawn (`webapp/editor.core.js:2535-2562`); automatic ones exist only inside `print::Plan` with no wasm binding | P1 | medium | V |
| **R-7** Flag values with arrows, traffic lights or ratings (icon sets) | yes | **no** — colour and formula rules only | yes, four icon groups | No `IconSet` variant in `CfRule` (`crates/casual-calc-model/src/sheet.rs:899-979`). Import records `iconSet` Omitted/NotRetained at `crates/casual-calc-import/src/lib.rs:1391-1441`, with `:167-181` explaining why it cannot simply be retained | P2 | medium | V |
| **R-8** Write a CF rule for blanks, errors, begins-with, a date period, or ≥ ≤ ≠ | yes | yes | yes — eleven rule types incl. Text, Date and Blank/Error | Panel offers seventeen operators (`webapp/editor.dialogs.js:1532-1544`); `NotEqualTo`, `NotBetween`, `GreaterThanOrEqual`, `LessThanOrEqual` **exist in the model and are unreachable from it**. `beginsWith`/`endsWith`/`containsBlanks`/`containsErrors` absent. `timePeriod` is kept as an equivalent formula and recorded Degraded (`lib.rs:1397-1430`) — good behaviour | P2 | medium | V |
| **R-9** Format a column as euros or a locale date without typing an OOXML code | yes — ~100 currencies, ~17 date types, 9 fraction types | yes — searchable custom pickers | yes | Fourteen presets at `webapp/editor.html:405-424`; every other currency and date layout is behind a free-text OOXML field. **Fractions do not render at all** — `crates/casual-calc-layout/src/numfmt.rs:14` defers them, `:606-611` pushes a space for `?`. numFmtId 12/13 are Excel *built-ins*, so this fires on files nobody customised | P2 | medium | V |
| **R-10** Give a chart series its own colour; keep an imported chart's colours | yes | yes | yes | `ChartSeries` has no colour field (`crates/casual-calc-model/src/chart.rs:130-179`); colours are `ACCENT_SLOTS[i % slots]` (`crates/casual-calc-layout/src/chart.rs:136-143`); the importer reads none (`grep -n 'spPr\|solidFill\|srgbClr' crates/casual-calc-import/src/chart.rs` → nothing). A branded chart is silently re-paletted. Retitling detaches the retained part (`objects.rs:591-604`), which *is* disclosed | P2 | medium | V |
| **R-11** Make a CF rule change a border or a number format | yes — full dxf | partial — no border, no number format | yes | `ConditionalFormat` carries fill, font colour, bold, priority, stop-if-true. A dxf stating only a border or numFmt is counted and the rule dropped (`crates/casual-calc-import/src/lib.rs:1455-1470`) | P2 | medium | V |
| **R-12** Restyle a workbook by theme; save a look as a named style | yes — themes + ~40 styles + New Cell Style | partial | partial | Ten built-in styles (`crates/casual-calc-wasm/src/io.rs:463-494`), workbook definitions winning (`:501-512`). No `session_define_cell_style`. Theme colours are **read from the file** and reach the picker (`crates/casual-calc-wasm/src/style.rs:116-124` → `webapp/editor.dialogs.js:399`); nothing writes them | P3 | medium | V |
| **R-13** Have a chart's vertical axis title actually appear | yes | yes | yes | The field exists (`webapp/editor.core.js:4455`) and the label is drawn nowhere. `crates/casual-calc-layout/src/chart.rs:732-737` reserves the space and comments "Reserved but not drawn: rotated text has no display-list variant". Same missing field as R-4 | P2 | medium | V |
| **R-14** Shrink a heading to fit its column | yes | no | unverified | **Renders already**; no control. `crates/casual-calc-wasm/src/axis.rs:815-817` emits it, `webapp/editor.core.js:3014-3020` honours it, no setter and no checkbox exist | P3 | small | V |
| **R-15** See that a cell holds a number forced to text | yes — green triangle | partial — alignment only | unverified | Flag is on the wire every frame (`crates/casual-calc-wasm/src/axis.rs:818-820`, `"qp":1`) and nothing consumes it. `docs/53` FC-15 🔴, the tracker's only open red row | P3 | small | V |

**Notes.** R-1 is the bucket's only `P0` and the only one where the product tells
the user the save succeeded while destroying their work. R-4's silent half is the
part that matters most: a rotated column header printing flat and overlapping its
neighbour is a wrong deliverable produced without a word, and `docs/12` §5 and
`docs/80` both make the stronger claim that this cannot happen. R-4, R-13 and
future RTL work all converge on one missing field — a rotation on
`PaintItem::Text` — and should be priced once as a display-list change, not three
times as three bugs.

### 4.3 Platform, data safety and extensibility — 15 gaps

| Gap, as a task | Excel | Sheets | ONLYOFFICE | OpenCalc today | Sev | Effort | Conf |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **P-1** Keep a version history for two workbooks that share a filename | yes — history binds to the cloud file id | yes — per Drive file id | yes — host owns identity | **The second one permanently deletes the first's.** `docKey()` is the display name alone (`webapp/editor.versions.js:77`); `store.put({ key: doc, versions: manifest })` at `:136-138` replaces the row; `:141-150` then deletes every byte row prefixed `${doc}:` the new manifest does not name. `forgetVersions()` is called only from File ▸ New (`webapp/editor.core.js:9636`). Renaming orphans a history; renaming onto an existing name arms the deletion. Drafts key by slot id and got this right (`webapp/editor.drafts.js:175`) | **P0** | small | V |
| **P-2** Edit a signed workbook and hand it back honestly | yes — removes the signature and says so | n/a | yes | **Worse than dropped: written back stale.** `crates/casual-calc-import/src/lib.rs:269-274` retains any part reachable from the package root — `_xmlsignatures/*` is, via `_rels/.rels` — and `crates/casual-calc-model/src/workbook.rs:31` re-emits retained parts verbatim. So the file carries a signature over content that no longer matches, and no loss path counts it. `SIGN-01` Open P2 | **P0** | small | V |
| **P-3** Open a password-protected workbook; set a password on one you send | yes, both | **no** — a long-standing limitation | yes, both | Neither. No CFB/ECMA-376 Part 2 handling anywhere, so an encrypted `.xlsx` falls out of admission as `PackageError::NotAPackage` (`crates/casual-calc-package/src/error.rs:24`, `OC-PKG-0005`) displayed as **"the input is not a valid ZIP/OPC package"** — telling a user their intact file is corrupt. The write-side refusal is deliberate and well reasoned (`crates/casual-calc-model/src/sheet.rs:336-343`) | P1 | large | V |
| **P-4** Automate anything — record steps, write a function, install somebody's add-in | yes — VBA + Office Scripts + Office.js + COM/VSTO | yes — Apps Script + triggers + Marketplace + Named Functions | yes — JS macros, `AddCustomFunction` (async since 9.0), plugin contract, in-editor marketplace, and a **macro recorder in the shipped source** | Nothing. No plugin/addon/add-in match across `crates/casual-calc-wasm/src/` and `webapp/*.js`; no script or eval export among the 294 bindings. Macros are retained and never executed, deliberately (`crates/casual-calc-sdk/src/lib.rs:77-88`) | P1 | very large | V |
| **P-5** Let named people edit one range; stop anyone adding sheets; password the protection | yes — 15 allowances + password + Allow Edit Ranges with user ACLs | yes — identity-based, plus a warning-only mode | yes — four levels, protected ranges as API objects with per-user rights | Four bindings total: cell locked/hidden and a sheet toggle. No workbook-structure export. `<protectedRanges>` is carried verbatim and never authored (`crates/casual-calc-model/src/sheet.rs:246-252`). A file's own fine-grained flags **are** honoured on read (`:325-334`, `permits(action)`) and nothing can set them | P1 | medium | V |
| **P-6** Keep working through a network drop and have the work merge back | partial — offline file edit, reconcile on reconnect | yes — Docs Offline | partial — desktop offline | Discarded outright on an unresumed reconnect, and **announced first**: `webapp/collab.js:311` consults `collab_unacknowledged()` and `:332` raises `status("lost", "unsentEdits")` before the snapshot lands. The comment at `:290-333` explains why nothing can save it. Resume works inside a bounded window (`server/.../document.rs:225-229`) | P1 | very large | V |
| **P-7** Generate or convert a spreadsheet from a server or a script | partial — Graph + Office Scripts | partial — Drive export + triggers | yes — Document Builder with 5 bindings, plus an HTTP conversion service | No artefact exists. `grep '^publish' crates/*/Cargo.toml` → `publish = false` for all sixteen, `casual-calc-sdk` included. The only workspace binary is the collab server. The npm packages are browser embedding surfaces | P2 | medium | V |
| **P-8** See every comment in a workbook in one place | yes — Comments task pane | yes — filterable sidebar | yes — sidebar with resolve-all | `session_comments(sheet, r0, c0, r1, c1)` is per-sheet and takes a rectangle (`crates/casual-calc-wasm/src/objects.rs:1366`). The only whole-document use is `selectCommentedCells` (`webapp/editor.core.js:7224-7238`) — Excel's Go To Special ▸ Notes, not a list. No comments panel among the nine | P2 | small | V |
| **P-9** Direct a comment at a person or hand them a task | yes — @mention + assign | yes — mention, assign, react | yes — mention notifies by email and Talk | Model is closed and has no field: `crates/casual-calc-model/src/sheet.rs:806-830` with `deny_unknown_fields` at `:812`. No mention/assignee match anywhere | P2 | medium | V |
| **P-10** Open a workbook you saved yesterday while offline | partial — desktop yes, web no | yes | partial | No service worker and no manifest in `webapp/`. Once loaded it is fully client-side and drafts persist, so an in-progress session survives a drop; a cold start does not. The Tauri shell is fully offline | P2 | small | V |
| **P-11** Have your version history follow the document to another machine | yes | yes, with per-person attribution | yes, host-served | Local IndexedDB only — `webapp/editor.versions.js:33`, `DB_NAME = "opencalc-versions"`. The store was deliberately built host-agnostic (`crates/casual-calc-wasm/src/versions.rs:49-107`), so this is a policy decision, not a mechanism gap. Same identity question as P-1 | P2 | large | V |
| **P-12** Save-a-copy or rename inside a WOPI host | yes — PutRelativeFile, RenameFile | n/a | yes — delegated to the host | Client implements CheckFileInfo, GetFile, Lock, RefreshLock, Unlock, PutFile (`server/casual-calc-wopi/src/wopi.rs:175/198/232/241/259/309`). No `PutRelativeFile`, no `RenameFile`. `wopi` preset sets `canSaveAs: false` (`webapp/editor.core.js:966`) | P2 | small | V |
| **P-13** Read or write a stored workbook's cells from another system over HTTP | yes — Graph workbook API | yes — Sheets REST API | partial — Automation API connector, no cell-level REST | The demo host says of itself "Not multi-tenant, not authenticated" (`server/casual-calc-host/src/main.rs:21-27`). There **is** a real outbound webhook — the signed token carries a save callback (`server/casual-calc-collab-server/src/token.rs:315`) — and nothing inbound | P2 | large | V |
| **P-14** Open a legacy `.xls` workbook | yes | yes | yes | Refused deliberately. `CANDIDATE_EXTENSIONS` is seven names (`crates/casual-calc-wasm/src/io.rs:249`) and `SessionFormat::for_extension` returns `None` rather than guessing (`crates/casual-calc-sdk/src/lib.rs:103-110`). `IO-04` Partial P1 records that the measurement of whether users still have such files has never been taken | P2 | very large | V |
| **P-15** Round-trip an `.ods` file without losing everything but the numbers | yes — ODF 1.4 | partial | yes — ODS/FODS/OTS/SXC all native | By design: "values, formulas, sheets and the document's own metadata, and nothing else" (`crates/casual-calc-ods/src/lib.rs:1-14`). Styles, merges, charts, images, CF, validation and print setup are **counted and named**, and the loss is askable before the save commits (`crates/casual-calc-wasm/src/io.rs:349`) | P2 | large | V |

**Notes.** Both `P0`s in this bucket are about *identity* — a document's, and a
file's — rather than about contents, which is why neither appears in any feature
matrix and why both survived several audits. Settling document identity once (a
stable id independent of the display name) closes P-1 and is the same decision
`docs/12` §10.2 item 2 is waiting on for P-11. P-3's diagnostic half is
separable from its decryption half and should not wait for it: sniffing the CFB
magic and answering "this workbook is password-protected" is one error variant.

### 4.4 Experience, reach and scale — 14 gaps

| Gap, as a task | Excel | Sheets | ONLYOFFICE | OpenCalc today | Sev | Effort | Conf |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **X-1** Type a date, or a number with a thousands separator | yes — locale date orders, month names, separators | yes — document-scoped locale | yes | **Becomes text, silently.** ISO only, then a bare `f64` parse, then intern as a string: `crates/casual-calc-sdk/src/lib.rs:1160`, `:1190`, `:1207`; `parse_date` hard-requires `len == 10` with dashes at 4 and 7 (`crates/casual-calc-io/src/lib.rs:249-253`). Probe, 2026-09-09: `12/31/2026 → None`, `31/12/2026 → None`, `31-Dec-2026 → None`, `1,234.5 → None`. Not a locale gap — a US Excel user hits it in the first minute | **P0** | medium | V |
| **X-2** Use the editor in a language other than English | yes — 130+ accessory packs; localised function names and separator | yes — UI plus ~22 localised function-name sets | yes — **46** catalogues shipped with this editor, 118 with the mobile one | Mechanism, no catalogues. `messages` is an empty Map (`webapp/editor.core.js:4901`); `availableLocales()` returns `["en-US", ...messages.keys()]` (`webapp/editor.i18n.js:83-84`). No `webapp/locales`, no pipeline. Function names English-only. The lexer emits only `Token::Comma` (`crates/casual-calc-formula/src/lex.rs:58`) — `=SUM(A1;A2)` cannot parse | P1 | large | V |
| **X-3** Open an Arabic or Hebrew workbook and have it look right | yes — sheet direction and per-cell text direction | yes in product; documentation thin | yes — ar/he/ur locales, RTL assets, an RTL stylesheet | Chrome pinned: `text-align: left` (`webapp/editor.css:58`) and `direction: ltr` (`:59`) inside the block that re-establishes inherited values after `all: initial`, so every descendant inherits it with no override path. 53 physical `left`/`right` declarations in the same file. `readingOrder` round-trips and does not render (`docs/53` FC-14 🟡) | P1 | large | V |
| **X-4** Start typing Japanese, Chinese or Korean into a selected cell | yes | yes | yes — ships ja/ko/zh/zh-tw | Nothing opens. The printable-key branch is `if (e.key.length === 1 && !mod) { startInline(e.key); … }` (`webapp/editor.core.js:8664`); a composition keydown arrives as `"Process"`, length 7. No `isComposing`, no `keyCode === 229`, no `"Process"` anywhere in `webapp/*.js`. The grid is a `<canvas>` with no composition target. **F2 first, then type, works** — the in-cell editor is a real `<textarea>` (`webapp/editor.html:563`) | P1 | medium | V |
| **X-5** Install the desktop app without being told it is damaged | yes — signed and notarised | n/a | yes — signed installers for three platforms | `"signingIdentity": "-"` (`desktop/tauri.conf.json:101`) is ad-hoc, not a Developer ID; no Windows `certificateThumbprint`. Tracked and **decided**, not overlooked — `TAURI-008` Open P1 carries the owner's 2026-08-30 decision verbatim, including that Apple's "damaged" wording misleads | P1 | small | V |
| **X-6** Edit a large workbook without it freezing | yes — native, parallel calc, save off the interactive path | yes — persistence is server-side | partial — same browser constraint, but the host owns the save | `session_save()` blocks the main thread **424–436 ms at 300k cells** against an 8 ms IndexedDB write (`webapp/editor.drafts.js:19-25`, restated `:420`). `grep -rn "new Worker" webapp/ sdk/ desktop/` returns nothing. Measured and named as `SAVE-06`, not hidden | P1 | large | V |
| **X-7** Say what happens at the scale the product promises | yes — capacity published to the row | yes — one published, enforced ceiling | no published editor figure | Grid is Excel-sized and real (`crates/casual-calc-formula/src/reference.rs:43-45`). Evidence is not: seven benchmark case ids defined, **two** baselined, both micro, none an eval case. The 1M / 60 fps / <50 ms figures in `CLAUDE.md` and `docs/30` are **design targets with no committed measurement** | P2 | medium | V |
| **X-8** Ask the application to do something by naming it | yes — Alt+Q executes ribbon commands | yes — Alt+/ | partial — Ctrl+/ is the shortcut reference, not command search | Absent. No palette match in `webapp/*.js` except one comment at `webapp/editor.core.js:6144` listing "the command palette" among text fields the shell must release accelerators for — a reference to something that does not exist | P2 | small | V |
| **X-9** Open the editor offline, or install it from the browser | no for web, yes for desktop | partial — Docs Offline extension | partial | No service worker, no manifest, no `theme-color`/`apple-touch-icon`/`color-scheme`. The head tags are already the named remainder of `UX-MOB-06` | P2 | medium | V |
| **X-10** Self-host into a Kubernetes cluster from what the repo provides | n/a | n/a | yes — and since 9.4 the AGPL Community edition dropped the 20-connection cap and the RabbitMQ/database dependencies | `deploy/` holds four web-server configs and two compose files. No chart, no manifests. `DEP-08` **Blocked** P1 — unblocked by code, blocked on there being a cluster to render against | P2 | medium | V |
| **X-11** Get an accessibility answer about a workbook before sending it | yes — Checker, Assistant, Accessibility ribbon | partial — no checker; explicit per-reader support and a braille mode | partial — screen reader beta since 7.5; "working toward" WCAG 2.1 AA, no conformance claim | No checker, no alt-text model (`grep alt_text\|altText\|descr` over `chart.rs` and `editor.charts.js` → nothing), no rule-based a11y harness in `tests/browser`, no `forced-colors` block. `prefers-contrast: more` **is** honoured (`webapp/editor.css:1886`) | P2 | medium | V |
| **X-12** Work on a phone or tablet | yes — first-party apps | yes — first-party apps | yes — apps plus a separate React mobile web build | A responsive page and a desktop binary. No PWA install (no manifest); `desktop/tauri.conf.json` targets desktop only. Touch work that landed is careful — handles shown on real `touchstart` rather than `(pointer: coarse)` (`webapp/editor.core.js:3775-3802`). Remainder tracked: `UX-MOB-06/07/08` | P2 | very large | V |
| **X-13** Reach Paste Special, row height or column width from the menu bar | yes, all three | yes, all three | yes — and 9.3 consolidated Format on the Home tab | The Edit menu has no Paste Special entry (`webapp/editor.core.js:9710-9738`, read in full); row height and column width exist only on the row/column **header** context menu (`webapp/editor.dialogs.js:2835`). The dialogs themselves are real and good — this is purely routing | P3 | small | V |
| **X-14** Zoom out far enough to see a wide sheet's shape | yes — 10%–400% | partial — a preset list | partial | `export const ZOOM_MIN = 0.25, ZOOM_MAX = 2;` — `webapp/editor.core.js:1588` | P3 | small | V |

**Notes.** X-2, X-3 and X-4 are *population exclusions* rather than degradations
— they make the product unusable for whole groups of people rather than worse for
everyone — which is why all three sit at `P1` above items with more users
affected. X-5 is the one item on the list that needs no design at all, only a
purchase and a CI secret. `UX-MOB-08` (mouse and touch landing on different
cells, `docs/14:157`) is filed as mobile polish and is a pointer landing
somewhere other than where it was aimed; the precedent for pulling it forward is
`UX-MOB-05`, which was a `P0` because a submenu off the right edge meant a tap
could delete data.

---

## 5. Engine capable, editor cannot reach it — the cheap class

`docs/12` §6 named this category and gave it seven instances. It is now the
single best effort-to-value ratio in this document, and it has grown. **Nine
gaps** below are cases where the engine already computes, transmits or accepts
the answer and no route exists to it.

`docs/12` §6 also carries a withdrawn sentence that is the standing warning here:
*"Every one of these is a host-side wiring job, not new engine work"* was **false
for three of seven**. So the question is never "does the engine do this" but **"is
there a binding that returns it"** — and each row below answers that specifically.

| Gap | What exists | What is missing | Cost |
| --- | --- | --- | --- |
| **R-3** chart subtypes | `ChartGrouping` (`crates/casual-calc-model/src/chart.rs:56-75`), `ChartSeries::kind` `:158`, `::secondary_axis` `:169`, `::data_labels` `:178`; wire fields **already accepted** at `crates/casual-calc-wasm/src/objects.rs:98/:130/:133`, deliberately `Option` so a panel that cannot say "stacked" cannot accidentally say "not stacked", with a test at `:2197-2210` | Four `<select>`s and two checkboxes in `buildChartPanel` (`webapp/editor.core.js:4418-4490`). **Trap:** `retunable_in_place` (`objects.rs:591-604`) makes grouping retunable but a *kind* change detaches the retained part, so send grouping, not a synthetic kind | hours |
| **C-11** iterative calculation | `WorkbookSettings::iteration()` (`crates/casual-calc-model/src/workbook.rs:252`), Excel's 100/0.001 defaults, three tests incl. early stop on convergence (`crates/casual-calc-eval/src/tests.rs:3943-4010`) | One wasm export; one field on the Calculation dialog. `grep -rn iterat crates/casual-calc-wasm/src/` returns nothing | hours |
| **R-14** shrink-to-fit | `Style::shrink_to_fit`; wire byte at `crates/casual-calc-wasm/src/axis.rs:815-817`; renderer at `webapp/editor.core.js:3014-3020` | One setter beside `session_set_rotation` (`crates/casual-calc-wasm/src/structural.rs:696`); one checkbox on the Alignment tab | hours |
| **R-15** quotePrefix indicator | Flag on the wire every frame — `crates/casual-calc-wasm/src/axis.rs:818-820`, `"qp":1`. Nothing in `webapp/*.js` reads it | One triangle in the cell painter, beside the existing per-cell drawing at `webapp/editor.core.js:3005-3065`. This is `docs/53` FC-15 | hours |
| **R-8** four CF operators | `NotEqualTo`, `NotBetween`, `GreaterThanOrEqual`, `LessThanOrEqual` all in `CfRule` (`crates/casual-calc-model/src/sheet.rs:914-921`) and evaluated | Four entries in the operator array at `webapp/editor.dialogs.js:1532-1544` and their mapping in `cfFromPanel`. The file can currently carry a rule the UI cannot author, so a workbook edited here silently narrows | hours |
| **R-6** automatic page breaks | `Page { rows, cols }` and `Plan { pages, … }` at `crates/casual-calc-layout/src/print.rs:1033-1070`, already consumed by the PDF path via `crates/casual-calc-sdk/src/lib.rs:2252` | One wasm export returning the page boundaries. `webapp/editor.core.js:2535-2562` already has the loop that would draw them | small |
| **C-12** Data ▸ Subtotal | `SUBTOTAL` evaluates with correct filter-visibility semantics; `session_group` and `session_show_outline_level` already build and drive the outline (`crates/casual-calc-wasm/src/axis.rs:1048-1179`) | A loop over a sorted key column that calls the two of them | small |
| **C-8** sort beyond three keys | `session_sort_range_multi` already takes a vector of keys (`crates/casual-calc-wasm/src/structural.rs:415-423`) | Delete the literal at `webapp/editor.dialogs.js:641`; add an "Add another sort column" button. (Colour and custom-list keys are a *different*, larger row — do not schedule them together) | small |
| **X-8** command palette | Every menu item and toolbar button funnels through one command id (`webapp/editor.core.js:1218-1230`), and `listCommands`/`run` over those same ids is **already public SDK surface** (`webapp/embed.d.ts`) | A filter over a list that is already enumerable | small |

Near-misses worth naming, because they look like this class and are not:

- **C-9 sheet-scoped names.** `DefinedName.sheet` exists and the importer uses
  it; `session_define_name` hardcodes `sheet: None`
  (`crates/casual-calc-wasm/src/objects.rs:1403`). Creating one is small.
  *Renaming with reference rewriting is not* — that is AST re-anchoring, the same
  shape of problem a multi-key sort already solves at
  `crates/casual-calc-wasm/src/structural.rs:431-436`.
- **R-1 `extLst`.** The *cheapest correct* fix is the one the model already uses
  elsewhere — carry it verbatim into `Sheet::carried` exactly as
  `<protectedRanges>` is carried — which restores the round trip without
  modelling sparklines at all. That is a genuine wiring job. Reporting it is
  cheaper still and is what `docs/34` requires today.
- **R-2 pictures.** `docs/12` §6 lists this as engine-capable. **Half right, and
  the half matters**: reading is engine-capable, writing is not. There is no
  `session_add_image` to wire up, and the PDF backend refuses `Image` outright
  (`crates/casual-calc-render/src/pdf.rs:883-889`).

**Together, the nine rows above are days.** They close one `P1` outright (R-3),
four more partially, and four `P2`/`P3` rows. That is a better first round than
starting any single large item.

---

## 6. Where OpenCalc leads

Evidence, not positioning. Each of these was checked this pass.

**Loss is accounted for before it happens, and the accounting is reachable.**
Every save path can be asked what it will cost before it runs —
`session_save_loss` and `session_save_loss_for(ext)`
(`crates/casual-calc-wasm/src/io.rs:331`, `:349`), `session_export_pdf_loss`
(`:399`), `session_import_summary` (`structural.rs:376`) — and the editor calls
them before a lossy save commits (`webapp/editor.sheets.js:642`, `:705`). Losses
are keyed **by construct**, not counted: `crates/casual-calc-import/src/lib.rs:167-198`
reports on the cfRule's own type, so a host can say "three iconSet rules and one
containsBlanks rule were dropped". `lib.rs:1397-1430` goes further and keeps a
`timePeriod` rule as the equivalent formula, painting the right cells and
recording it Degraded because it will read as "Use a formula" when reopened in
Excel. All three competitors drop things on cross-format saves and say nothing;
Google does not document the ARRAYFORMULA export hole anywhere a user will see
it. **R-1 is the one place this promise is currently broken, which is exactly why
it is a `P0`.**

**An unedited file saves as itself, byte for byte**
(`crates/casual-calc-sdk/src/lib.rs:2004-2010`). None of the three does this.
ONLYOFFICE converts every opened file to XLSX internally
(`helpcenter.onlyoffice.com` supported-formats, 2026-09-09).

**A real per-cell accessibility tree over a canvas grid.**
`webapp/editor.core.js:6728/6733/6746/6753` emit `columnheader`, `rowheader` and
`gridcell` roles; only the visible window is mirrored (60×40, `:6583-6584`) but
every node carries its **absolute** index against the sheet's declared counts
(`:6571-6573`), so "row 4,201 of 1,048,576" stays true with 40 rows in the DOM.
An empty cell gets an explicit `aria-label` "B7 empty" so readers do not skip it
and misalign the row (`:6757-6759`). Sheets and Excel for the web get this by
rendering a DOM grid; getting it over a canvas at 60 fps is the harder version.
The rebuild is deferred **only while the view is moving**, because a signature
hashing every visible cell changes every frame during a scroll — a profile put
43% of the frame in two functions that draw nothing (`:6592-6605`).

**Full LAMBDA family with real spill semantics.** LAMBDA, LET, MAP, REDUCE, SCAN,
BYROW, BYCOL, MAKEARRAY, ISOMITTED all dispatch (parsed from `FUNCTIONS`,
`crates/casual-calc-eval/src/functions/mod.rs:59`), with `SPILL_ANCHOR`/`SPILL_CHILD`
flags and `#SPILL!` blocking (`crates/casual-calc-model/src/cell.rs:19-21`,
`crates/casual-calc-eval/src/lib.rs:368-418`). **ONLYOFFICE has none of these** —
LET and LAMBDA are open community requests as of October 2025. And a *shrinking*
spill is cleared before it is re-laid (`crates/casual-calc-eval/src/lib.rs:348`,
`:363-372`); ONLYOFFICE shipped dynamic arrays in 9.3 with exactly that defect
open (`DocumentServer#3576`, seen 2026-09-09). We are ahead of the closest
architectural peer on a feature they shipped seven months ago.

**A stopped recalculation says so.** `session_recalculate` returns `"full"` /
`"cancelled"` / `"over-budget"` (`crates/casual-calc-wasm/src/calc.rs:88-108`)
and the editor answers "calculation stopped — some values are still out of date"
(`webapp/editor.sheets.js:449-456`). Excel shows "Calculating…" and then stops
showing it. Presenting a half-fresh sheet as final is the one thing a host must
not do, and this is the only one of the four that provably refuses to.

**Personal filter views, placed better than either incumbent's.**
`session_set_personal_filter` hides rows for one participant only
(`crates/casual-calc-wasm/src/view.rs:10-37`), reached from the filter dropdown
as "Just for me" rather than from a separate mode. Excel's Sheet Views require
SharePoint/OneDrive and cap at 256; Sheets' Filter Views are a menu away. And
the boundary is reasoned about: a personal filter does not move a `SUBTOTAL`, a
shared one does, with a test that fails if the shared case stops moving it
(`crates/casual-calc-sdk/src/tests.rs:1918-1960`). None of the three documents
what its per-user filtering does to an aggregate underneath.

**Session access control enforced at the operation level, deny-by-default.** A
Comment-level token's cell edit is refused by the server's match, not by hiding a
toolbar, and an operation added later is refused for everyone below Edit until
somebody decides which side it belongs on
(`server/casual-calc-collab-server/src/token.rs:59-70`).

**A collaboration server with no per-document state and no I/O.** Every module is
a state machine over supplied time and supplied bytes
(`server/casual-calc-collab-server/src/lib.rs:1-23`), which is why the save
cadence and retry policy are testable at all.

**Loss on reconnect is announced before it happens.** `webapp/collab.js:296-333`
consults `collab_unacknowledged()` and names the loss *before* the snapshot
lands, distinguishing "the server forgot us" from "this client abandoned a state
it cannot defend". Nobody else in this category tells a user which of their edits
did not survive.

**Non-bypassable package limits as a shared substrate.** One admission layer with
explicit `PackageLimits` under both `.xlsx` and `.ods`, per-part XML element and
depth caps above it, stable `OC-PKG-*` diagnostic codes
(`crates/casual-calc-package/src/error.rs:30-43`), and a fuzz corpus. Zip-bomb
and XML-bomb resistance is structural, not remembered.

**Apache-2.0 against ONLYOFFICE's AGPL-3.0** (`LICENSE:1-3`). ONLYOFFICE's own
licence FAQ states that making an AGPL build available over a network obliges you
to publish your modifications and derivative work — which is what Developer
Edition exists to relieve. In the self-hosting contest, which is the contest with
ONLYOFFICE, this is the structural advantage. **It converts into wins only if
there is a deployment path, which is X-10, which is Blocked.** That is the one
strategic coupling in this document worth naming to the owner.

**Deterministic output.** Caller-supplied clocks (`session_set_clock`; comment
`created` supplied rather than read, `crates/casual-calc-model/src/sheet.rs:820-824`),
fixed workbook ids, insertion-ordered interning. Building the same workbook twice
produces the same bytes, which is what makes golden-file testing possible and is
true of none of the three.

**Bytes cross the desktop bridge, never paths** — no shell command accepts or
returns a path, so a webview cannot ask the process to read an arbitrary file
(`desktop/src/main.rs:22-25`).

**Honest, self-correcting documentation of its own gaps.** `docs/12` opens by
naming the six times its own citations were quoted rather than opened; `docs/53`
tracks correctness through five separate questions so "passes UI, fails RENDER"
is a different row from "fails UI"; `SAVE-06`'s 424 ms hitch is named in the file
that causes it. None of the three publishes anything comparable. Strategically
this is worth more than it looks: it is why a round like this one can be run at
all.

---

## 7. The switching-blocker list

**Sixteen of fifty-eight.** Ranked by whether it stops a user moving here today,
not by cost to fix — the same rule `docs/12` §8 uses, and for the same reason:
re-sorting by cost silently loses the argument.

1. **X-1 — a typed date becomes text.** First minute, every user, silent, and it
   reaches the file. Nothing else on this list is hit sooner.
2. **R-1 — an edited save destroys sparklines and every other `extLst`
   construct.** The user is told the save succeeded.
3. **P-1 — a second workbook with the same filename deletes the first's version
   history.** In the feature built to prevent losing work.
4. **C-1 — an Excel table's calculated columns open as errors, and
   `[#This Row]` returns a plausible wrong number.** Arrives through import; the
   user does nothing unusual to hit it.
5. **P-2 — a signed workbook is handed back carrying a stale signature.** Reads
   to the recipient as tampering.
6. **P-3 — a password-protected workbook cannot be opened, and the error says
   the file is corrupt.** For a finance or HR evaluator this is file one.
7. **X-2 — nothing ships translated, and `=SUM(A1;A2)` cannot parse.** Excludes
   every non-English team.
8. **X-4 — a CJK user cannot begin typing into a selected cell.** F2 first works,
   which nobody will discover.
9. **X-3 — Arabic and Hebrew workbooks lay out wrong and the chrome is pinned
   LTR.**
10. **P-4 — no automation of any kind.** Excludes an entire class of workbook
    outright, and all three competitors have two or more answers each.
11. **C-2 — no Goal Seek, Solver, Scenario Manager or Data Table.** A finance
    user's workbook contains one and there is no route to it.
12. **R-2 — no way to put a picture on a sheet.** Day one for anyone making an
    invoice or a report.
13. **P-5 — protection is an honour system.** No password, no allowance list, no
    per-range permissions.
14. **R-4 — printing and PDF do not reproduce the sheet, and half the loss is
    unreported.** For many users the printout *is* the deliverable.
15. **X-5 — the desktop app reports itself as damaged on install.** A decision,
    not a defect, and the cheapest item here to reverse.
16. **C-3 — a pivot cannot express a share.** The first thing reached for after
    Sum. **C-4** (external data) sits just behind it for the same class of user.

**R-3** (chart subtypes) is deliberately *not* on this list any more. It was
`docs/12` §8 item 6, and it stopped being a blocker when `CHT-05`/`CHT-16`
landed — the capability is in the file and in the engine and only the panel
cannot ask for it. That is a different, much cheaper problem, and leaving it
ranked as a blocker would misprice the round.

---

## 8. What parity would take

### 8.1 Understood — needs time, not a decision

Ordered by value per unit of work, which is not the order of §7.

1. **The nine engine-capable rows in §5.** Days. Closes R-3 outright and moves
   C-8, C-11, C-12, R-6, R-8, R-14, R-15 and X-8. Start here.
2. **X-1, the date and number entry policy.** One function — `input_edit`
   already owns the whole "what does typed text mean" rule
   (`crates/casual-calc-sdk/src/lib.rs:1102`, and its doc comment says it was
   moved into the engine precisely so a second host would not reimplement it), so
   a date-order and separator policy beside the ISO branch reaches every host at
   once.
3. **R-1, `extLst`.** Carry it verbatim into `Sheet::carried`, or at minimum
   report it. The reporting half is hours and is what `docs/34` already requires.
4. **P-1, a stable document key.** Content-derived or generated, stored with the
   document, plus a migration for existing buckets. No engine change.
5. **P-2, strip a signature on any save that changed the model** — and name it
   in the loss report. Creating signatures is a separate, much larger row that
   needs a certificate story this project does not have.
6. **C-1, the current-row specifier forms.** `Evaluator::current_cell()` is
   already used by the unqualified-`[Col]` branch (`eval.rs:192-201`), so the row
   is in hand; split a multi-part specifier and handle `@` / `#This Row`.
7. **X-13, X-14 — three menu lines and two constants.** An hour, collectively.
   `docs/12` §4.2 is right that this is what "feels like Excel" is made of.
8. **X-9, a service worker and the head tags.** ~100 lines, and it turns the
   7.8 MB payload from a repeated cost into a one-time one. The head tags are
   already the named remainder of `UX-MOB-06`.
9. **C-5, Text to Columns as an engine operation** with a qualifier, per-column
   types and an occupancy check — copying the care Remove Duplicates already
   takes two dialogs away (`webapp/editor.selection.js:864-886`).
10. **C-2's Goal Seek slice.** A bracketed root-finder already exists for the
    IRR family (`crates/casual-calc-eval/src/functions/financial.rs:255`) and
    `session_recalculate` with a time budget is the bounded loop it needs.
    Solver and Scenario Manager are not this.
11. **X-2's catalogues and separator.** Three halves with three different costs:
    catalogues (host-side, mechanical, needs translators not engineers), the
    argument separator (engine, small, disproportionately visible), localised
    function names (engine, large, and a fidelity hazard because the file format
    stores canonical English). Schedule them apart.
12. **P-5's allowance flags.** `SheetProtection` is an untyped attribute map
    (`crates/casual-calc-model/src/sheet.rs:301-306`) that already round-trips
    every OOXML allowance and already interprets them on read via `permits()`.
    Writing them is a dialog and a setter. Only the password hash and
    protected-range authoring are new engine work.
13. **C-9's rename, C-10's column chooser, C-8's key cap, P-8's comments panel,
    P-12's two WOPI operations.** Each small, each closing a visible edge.
14. **R-9's currency and date pickers.** The formatter already renders
    `[$SYM-locale]`, the eight bracketed colours, elapsed time, `_` pad and `*`
    fill — only the picker is missing, and `format_preview` already exists to
    drive it. *Fractions are separate and are a wrong number on screen.*
15. **X-7's baselines.** The harness exists and `docs/30` specifies the shape;
    what is missing is a committed baseline on a named machine.
16. **R-4's charts-and-CF half for `Ctrl+P`.** Engine-capable — the PDF path
    proves it by calling `layout_range` on the same workbook; the HTML writer
    just never asks. *The text-attribute half is not, and is item 3 in §8.2.*

### 8.2 Genuinely unsolved here — needs a decision first

All seven of `docs/12` §10.2 stand. Restated with what this pass adds:

1. **Concurrent editing correctness.** `COL-50`: an insert meeting a delete does
   not converge for a formula *range*, and each answer is the one Excel gives for
   its own order. Still the reason `canShare` is off in embedded and host modes.
   Unchanged.
2. **What a document *is* when there is no server.** P-1 and P-11 are the same
   question arriving twice — one as a `P0` defect, one as a portability gap.
   Settling document identity once answers both. This is now the most urgent of
   the seven, because it has produced a data-loss bug rather than only an absence.
3. **A display-list model for text attributes.** R-4, R-13 and any future RTL or
   vertical-script work all block on the same missing fields on `PaintItem::Text`.
   Adding rotation alone changes what "one display list, three executors" means
   and touches both backends. Price it once.
4. **The extensibility position.** P-4. What language, what capability set, how
   bounded, whether it survives `unsafe_code = forbid` and no-network. Every
   competitor has an answer and none is obviously portable. *Not executing
   untrusted VBA is the right position and should not be traded away to close
   this.* The smallest thing here that would change an evaluation is a **command
   recorder over the ids the menus and the SDK already share** — orders of
   magnitude cheaper than the category.
5. **IME and complex text.** X-4 is narrower and cheaper than `docs/12` §3.1
   implies — one keydown branch plus a composition target — but the *font
   coverage* half (`P1C-003`, Partial) is not: bundled faces cover Latin and
   Hebrew, not Arabic, Devanagari, Thai or CJK, and shaping is off in the WASM
   build by decision (`ADR-018`). Together they decide whether the product works
   for CJK and Indic users at all.
6. **Whether 7.8 MB of WASM is acceptable on a real network.** Never measured off
   localhost. X-9's service worker changes the answer, which is a reason to do it
   before measuring rather than after.
7. **Fidelity beyond the fixture corpus.** `IO-12` is the answer being built —
   a compatibility-outcome vocabulary in which "a file Excel opens after silently
   repairing it" stops passing every check here.

One item to add to the seven:

8. **X-6, the main thread.** `docs/90` records that the SDK API is already async
   on every workbook-touching method, so the interface a worker needs is in
   place. The move is still a project: the wasm module, the display-list bridge
   and the collab client all assume the main thread. This is a design
   conversation, not a schedule.

---

## 9. Proposed tracker rows

**These are proposals, not rows.** `docs/14` is being written to by another
workflow as this document is produced, and this document does not touch it.

**Ids must be assigned by `tools/check-tracker-ids.py` at filing time.** Do not
copy the `<assign>` placeholders. Five rows have already been added with an id
already in use and `FID-13` named two unrelated changes for weeks; the check is a
pre-commit hook and a CI gate for that reason.

**Prefixes proposed** — all existing except one: `CALC`, `DATA`, `FMT`, `CHT`,
`IO`, `FID`, `UX`, `SAVE`, `SDK`, `A11Y`, `DEP`, `DOC`, and **`I18N`, which does
not yet exist** in either tracker and is proposed for the internationalisation
rows (X-2, X-3, X-4). If a new prefix is unwelcome, `UX` absorbs them.

**Already tracked — do NOT file a duplicate.** These gaps have rows:
C-3 → `PIV-07`; C-4 partially → nothing (genuinely untracked, filed below);
P-2 → `SIGN-01` (**amend the mechanism**, see below); P-6 → `COL-50` adjacent;
P-14 → `IO-04`; R-4's PDF residuals → `IO-03`/`IO-10`/`IO-11`; R-3's engine half
→ `CHT-05`; X-5 → `TAURI-008`; X-6 → `SAVE-06`; X-10 → `DEP-08`; X-12 →
`UX-MOB-06/07/08`; X-7's benchmark question → adjacent to `IO-12`.

### 9.1 P0

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| `<assign>` CALC | A typed date or separated number becomes text | Open | P0 | `input_edit` infers only ISO (`crates/casual-calc-sdk/src/lib.rs:1160`) then a bare `f64` parse (`:1190`), else interns a string (`:1207`); `parse_date` requires exactly `YYYY-MM-DD` (`crates/casual-calc-io/src/lib.rs:249-253`). `12/31/2026`, `31/12/2026`, `31-Dec-2026` and `1,234.5` all store as text, sort wrong, break date functions, and reach the `.xlsx` as strings where Excel wrote dates. The only cue is left alignment | A test that types each of `12/31/2026`, `31/12/2026`, `31-Dec-2026`, `1,234.5` under a declared input policy and asserts `CellValue::Number` with a date format, plus a round-trip asserting the saved cell is a date in the written XML |
| `<assign>` FID | An edited save destroys worksheet `<extLst>` | Open | P0 | `extLst` is parsed nowhere and written nowhere — the single match in the import/export surface is a test allowlist (`crates/casual-calc-import/src/tests.rs:614`) — and it is not in `Sheet::carried` (`crates/casual-calc-model/src/sheet.rs:246-253`). Sparklines, x14 icon-set rules, x14 data validations and slicer references are lost on the first edited save. An unedited file saves byte-identically, so the loss fires only after an edit, and no `report.record` call site covers an unrecognised worksheet element | A fixture with a sparkline group: import → edit one unrelated cell → export → re-import asserts the `extLst` bytes are present and unchanged; and, until it is carried, a test asserting the compatibility report names it |
| `<assign>` SAVE | Two workbooks with the same filename share one version bucket, and the second deletes the first's | Open | P0 | `docKey()` is the document's display name alone (`webapp/editor.versions.js:77`); `store.put({ key: doc, versions: manifest })` (`:136-138`) replaces the whole row and the prefix sweep at `:141-150` then deletes every byte row the new manifest does not name. `forgetVersions()` is called only from File ▸ New (`webapp/editor.core.js:9636`), so Open never clears the bucket. Renaming orphans a history; renaming onto a used name arms the deletion. `webapp/editor.drafts.js:175` keys by rotating slot id and does not have this | A browser test: open A named `X`, capture a version, reload, open B named `X`, capture, reload, reopen A — A's versions are still listed and restorable |
| `<assign>` FID | A signed workbook is written back carrying a stale signature | Open | P0 | Amends **`SIGN-01`**, which says "silently invalidated". `crates/casual-calc-import/src/lib.rs:269-274` retains any part reachable from the package root — `_xmlsignatures/*` is, via `_rels/.rels` — and retained parts are re-emitted verbatim (`crates/casual-calc-model/src/workbook.rs:31`). The signature is not dropped; it is written over changed content. A present-but-broken signature reads to a recipient as tampering. No loss path counts it | A signed fixture: import → change one cell → export asserts the signature part is absent **and** named in `session_save_loss`; an unedited round trip asserts it is still present and byte-identical |
| `<assign>` CALC | Table current-row references fail, and `[#This Row]` returns the wrong number | Open | P0 | `crates/casual-calc-eval/src/eval.rs:215-221` matches the specifier as one raw string against four literals and falls to `_ => (first_data, last_data)`; `:224` sends anything starting `#` to the full column span, so `SUM(Sales[#This Row])` returns the whole table's total; `:227-230` returns `None` for `@Amount` and for `[[#This Row],[Amount]]`, giving `#REF!`. `Sales[Amount]` works and is tested (`tests.rs:1332`), so `docs/12` §3.15 and `docs/73` #10 are both wrong about which half is broken | An eval test over a two-row fixture table asserting `SUM(Sales[@Amount])`, `SUM(Sales[[#This Row],[Amount]])`, `SUM(Sales[[#Data],[Amount]])` and `[@Amount]*2` against hand-computed values, with a `SUM(B2:B3)` control in the same run |

### 9.2 P1

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| `<assign>` CHT | The chart panel cannot ask for stacked, combo, secondary axis or data labels | Open | P1 | Engine, model and wire all carry them — `ChartGrouping` (`crates/casual-calc-model/src/chart.rs:56-75`), `ChartSeries::kind`/`secondary_axis`/`data_labels` (`:158`/`:169`/`:178`), accepted at `crates/casual-calc-wasm/src/objects.rs:98`/`:130`/`:133`. `CHART_KINDS` at `webapp/editor.core.js:4404-4407` is seven kinds and `grep 'grouping\|secondary\|dataLabels' webapp/editor.core.js` returns nothing. Retuning must send grouping, not a synthetic kind, or `retunable_in_place` (`objects.rs:591-604`) detaches the retained part | A browser test that inserts a column chart, sets grouping to `stacked` and a series to the secondary axis, and asserts the exported `.xlsx` carries `<c:grouping val="stacked"/>` and a second `valAx` |
| `<assign>` I18N | No locale catalogue ships, and the formula lexer accepts only `,` | Open | P1 | `messages` is an empty Map (`webapp/editor.core.js:4901`); `availableLocales()` returns `["en-US", ...messages.keys()]` (`webapp/editor.i18n.js:83-84`); there is no `webapp/locales` and no pipeline. Separately, `crates/casual-calc-formula/src/lex.rs:58` emits only `Token::Comma`, so `=SUM(A1;A2)` cannot parse for any user whose locale trained them on `;` | Two gates, filed as two rows if scheduled apart: (a) a second catalogue loads and `relabel()` retranslates the menubar including mnemonics; (b) a parse test asserting `=SUM(A1;A2)` evaluates under a declared separator policy and still round-trips as `,` in the file |
| `<assign>` I18N | The editor chrome is pinned left-to-right | Open | P1 | `text-align: left` and `direction: ltr` are set on the shadow root inside the block that re-establishes inherited values after `all: initial` (`webapp/editor.css:58-59`), so every descendant inherits with no override path; 53 physical `left`/`right` declarations in the same file. Content-side `readingOrder` round-trips and does not render (`docs/53` FC-14) | A browser test that sets an RTL locale and asserts the menubar, panels and formula bar mirror, plus a CSS gate failing on any new physical `left`/`right` in `webapp/editor.css` |
| `<assign>` I18N | An IME composition keystroke does not open the cell editor | Open | P1 | The grid's printable-key branch is `if (e.key.length === 1 && !mod)` (`webapp/editor.core.js:8664`); a composition keydown arrives as `"Process"` and is dropped. No `isComposing`, `keyCode === 229` or `"Process"` handling exists in `webapp/*.js`, and the grid is a `<canvas>` with no composition target. F2 first works, because the in-cell editor is a real `<textarea>` (`webapp/editor.html:563`) | A browser test dispatching a `compositionstart` at the grid and asserting the in-cell editor opens and receives the composed text; a manual IME pass recorded on the row, since synthetic composition is not the same as an IME |
| `<assign>` IO | An encrypted workbook is refused as a corrupt package | Open | P1 | No CFB/ECMA-376 Part 2 handling exists, so an encrypted `.xlsx` — a compound file, not a ZIP — falls out of admission as `PackageError::NotAPackage` (`crates/casual-calc-package/src/error.rs:24`, `OC-PKG-0005`), rendered as "the input is not a valid ZIP/OPC package". The message tells a user their intact file is broken. The diagnostic half is separable from decryption | An import test over an encrypted fixture asserting a distinct error variant and registry code whose message names password protection, not package validity |
| `<assign>` DATA | Text to Columns is a UI string split that overwrites adjacent data silently | Open | P1 | `const parts = text.split(delim)` (`webapp/editor.dialogs.js:2210`) then `session_set_cell` per part (`:2212-2214`) with no bounds check and no occupancy check; quoted fields split wrong and there is no qualifier, fixed width, per-column type or destination. `grep -rn text_to_columns crates/` returns nothing — unlike Remove Duplicates, this never reached the engine, so it is also not undoable as one operation and not reachable from the SDK | An engine test splitting `"Smith, John",42` into two columns, and a test asserting the operation refuses (or prompts) when the destination is occupied, matching `webapp/editor.selection.js:864-886` |
| `<assign>` UX | No route to insert, select, move or delete a picture | Open | P1 | The complete Insert menu (`webapp/editor.core.js:9800-9817`) has no Image item, and the only image bindings are read-only: `session_images` (`crates/casual-calc-wasm/src/view.rs:416`) and `session_image_data` (`objects.rs:1505`). Painted at `webapp/editor.paint.js:85-106` with no hit-test and no handles. The anchoring model is shared with charts (`crates/casual-calc-model/src/chart.rs:360-386`); what is missing is an ingest path and the selection layer. The PDF backend also refuses `Image` (`crates/casual-calc-render/src/pdf.rs:883-889`) | Insert a PNG, move and resize it, save, reopen — the image is at the new anchor; and a PDF export containing it |
| `<assign>` IO | `Ctrl+P` and PDF export produce different pages and different content, and half the PDF loss is unreported | Open | P1 | Two paths. `session_print_html` states its own omissions (`crates/casual-calc-wasm/src/objects.rs:1820-1823`) — no charts, images or conditional formatting — and its CSS pins `vertical-align: bottom` with `white-space: pre`. The PDF path uses `print::Plan` and carries charts and CF, but `PaintItem::Text` (`crates/casual-calc-layout/src/display.rs:221-246`) has no rotation, wrap, vertical-alignment, underline, strikethrough or indent field, so those six flatten in both PNG and PDF while the canvas draws them. `session_export_pdf_loss` names only the pictures | A golden test comparing a fixture's canvas display list against its PDF display list for a rotated, wrapped, underlined, vertically-centred cell — equal, or named in the loss report |
| `<assign>` UX | Sheet protection has no allowance list, no password and no protected-range authoring | Open | P1 | Four bindings total: `session_set_cell_protection`/`session_cell_protection` (`crates/casual-calc-wasm/src/sheet.rs:271`/`:294`) and `session_set_sheet_protected`/`session_sheet_protected` (`view.rs:655`, `style.rs:37`). No workbook-structure export among the 294. `<protectedRanges>` is carried verbatim and never authored (`crates/casual-calc-model/src/sheet.rs:246-252`). A file's own flags **are** honoured on read via `permits()` (`:325-334`), so the model round-trips what nothing can set | Protect a sheet with "insert rows" allowed and "format cells" refused; assert the two operations behave differently and the exported `<sheetProtection>` carries both attributes |
| `<assign>` DATA | No what-if analysis of any kind | Open | P1 | `grep -rni "goal seek\|solver\|scenario manager\|data table"` over `webapp/ crates/` matches only an unrelated `resolver` and one comment in `functions/financial.rs:255`. The Data menu is complete at `webapp/editor.core.js:9885-9925`. Goal Seek is the cheap slice: a bracketed root-finder already exists for the IRR family and `session_recalculate` with a time budget is the bounded loop it needs | Goal Seek over a fixture reaching a target within a declared tolerance and iteration cap, and refusing (rather than hanging) on a non-convergent input |
| `<assign>` IO | Automatic page breaks are computed and not exposed | Open | P1 | `Page` and `Plan` exist at `crates/casual-calc-layout/src/print.rs:1033-1070` and the PDF path consumes them via `crates/casual-calc-sdk/src/lib.rs:2252`, but `grep -rn 'PagePlan\|paginate' crates/casual-calc-wasm/src/` finds only prose — no binding. `session_page_breaks` (`view.rs:344-368`) returns only user-set breaks, and `webapp/editor.core.js:2535-2562` already has the loop that would draw the rest | A browser test asserting the drawn automatic break positions equal `Plan`'s page boundaries for a fixture at a given scale and paper size |

### 9.3 P2 / P3

| ID | Title | St | Sev | Mechanism | Gate |
| --- | --- | --- | --- | --- | --- |
| `<assign>` CALC | Iterative calculation is implemented and cannot be turned on | Open | P2 | `WorkbookSettings::iteration()` reads `iterate`/`iterateCount`/`iterateDelta` with Excel's 100/0.001 defaults (`crates/casual-calc-model/src/workbook.rs:252`) and is tested including early stop on convergence (`crates/casual-calc-eval/src/tests.rs:3943-4010`). `grep -rn iterat crates/casual-calc-wasm/src/` returns nothing and Tools ▸ Calculation offers only Automatic / Manual / Calculate now. `docs/51` also lists this as unimplemented, which is wrong | Set iteration from the UI, build a circular model, assert it resolves; and assert the setting reaches `<calcPr>` in the written file |
| `<assign>` FMT | Four conditional-format operators exist in the model and are unreachable | Open | P2 | `NotEqualTo`, `NotBetween`, `GreaterThanOrEqual`, `LessThanOrEqual` are in `CfRule` (`crates/casual-calc-model/src/sheet.rs:914-921`) and evaluated; the panel's operator list (`webapp/editor.dialogs.js:1532-1544`) has seventeen entries and none of the four. A workbook carrying one of these rules, edited here, silently narrows to what the UI can express | A round-trip test authoring each of the four from the panel and asserting the written `<cfRule>` operator, plus a re-import asserting the rule survives an unrelated edit |
| `<assign>` FMT | No shrink-to-fit control | Open | P3 | `Style::shrink_to_fit` exists, `crates/casual-calc-wasm/src/axis.rs:815-817` emits it and `webapp/editor.core.js:3014-3020` honours it — there is no setter (`grep shrink crates/casual-calc-wasm/src/style.rs structural.rs` → nothing) and no checkbox anywhere in the dialogs or markup | Set shrink-to-fit from Format Cells, assert the cell renders scaled and the attribute reaches `<alignment shrinkToFit="1"/>` |
| `<assign>` FMT | A text-forced cell has no indicator | Open | P3 | `crates/casual-calc-wasm/src/axis.rs:818-820` puts `"qp":1` on the wire every frame and nothing in `webapp/*.js` reads it. This is `docs/53` FC-15, the only open red row | A browser test asserting the marker is drawn for a `quotePrefix` cell and absent otherwise, in both themes |
| `<assign>` DATA | Data ▸ Subtotal does not exist | Open | P3 | `SUBTOTAL` evaluates with correct filter visibility and `session_group`/`session_show_outline_level` already build and drive the outline (`crates/casual-calc-wasm/src/axis.rs:1048-1179`); nothing generates them. The Data menu (`webapp/editor.core.js:9885-9925`) has no entry, and there is no Auto Outline | Subtotal a sorted three-group fixture; assert the inserted formulas, the outline levels, and that collapsing to level 1 shows only the subtotal rows |
| `<assign>` DATA | Custom sort caps at three keys | Open | P2 | `[0, 1, 2].forEach(addKeyRow)` — `webapp/editor.dialogs.js:641`. `session_sort_range_multi` already takes a vector (`crates/casual-calc-wasm/src/structural.rs:415-423`), so the cap is entirely in the dialog. Sort by colour and by custom list are a **separate** row — the binding has no place to put either | A sort on four keys produces the hand-computed order |
| `<assign>` DATA | Remove Duplicates compares the whole rectangle | Open | P2 | `session_remove_duplicates(sheet, first_row, c0, r1, c1)` has no column mask (`crates/casual-calc-wasm/src/clipboard.rs:1295-1301`), so "duplicate by customer id, keep the other columns" is inexpressible. The adjacent-data prompt (`webapp/editor.selection.js:871-886`) is working around exactly this | Remove duplicates keyed on one column of a three-column fixture; assert the retained rows and that the untouched columns are unchanged |
| `<assign>` DATA | The Name Manager cannot rename, re-point or scope a name | Open | P2 | Two controls per name — go-to (`webapp/editor.dialogs.js:2257-2269`) and delete (`:2270-2283`). Four name bindings and none is an update; `session_define_name` hardcodes `sheet: None` (`crates/casual-calc-wasm/src/objects.rs:1403`) although `DefinedName.sheet` exists and the importer uses it. Renaming today means delete-and-redefine, which turns every user into `#NAME?` | Rename a name used by three formulas; assert the formulas still evaluate and the written `<definedName>` carries the new name. Sheet-scoping is a second gate: two sheets each define `Total` and each resolves locally |
| `<assign>` FMT | Fractions do not render, and there is no currency or date picker | Open | P2 | `crates/casual-calc-layout/src/numfmt.rs:14` defers fractions and `:606-611` pushes a space for `?`, so numFmtId 12/13 — Excel **built-ins** — display wrong with no report. Separately, the format menu is fourteen presets (`webapp/editor.html:405-424`) and everything else is a free-text OOXML field, although the formatter already renders `[$SYM-locale]`, the bracketed colours and elapsed time | A format test asserting `# ?/?` over `0.5` renders `1/2`; and a UI test choosing a non-dollar currency and a non-ISO date layout without typing a code |
| `<assign>` CHT | An imported chart is re-paletted silently and a series cannot be coloured | Open | P2 | `ChartSeries` has no colour field (`crates/casual-calc-model/src/chart.rs:130-179`); `series_color` takes `ACCENT_SLOTS[i % slots]` (`crates/casual-calc-layout/src/chart.rs:136-143`); the importer reads no `spPr`/`solidFill`/`srgbClr` anywhere in its 821 lines. The file is preserved and the *screen* lies, which is the opposite of the usual direction | Import a chart with an explicit series fill; assert the drawn colour equals the file's, and that a colour set in the panel reaches the written `spPr` |
| `<assign>` CHT | The chart vertical axis title is accepted and never drawn | Open | P2 | `webapp/editor.core.js:4455` offers the field; `crates/casual-calc-layout/src/chart.rs:732-737` reserves the space and comments "Reserved but not drawn: rotated text has no display-list variant". Since `RND-10` the canvas paints the same display list, so it now appears in **no** backend — and `chart.rs`'s own module comment saying the canvas draws it is stale. Blocked on the same missing `PaintItem::Text` rotation as the print row | The y-axis title appears in canvas, PNG and PDF for the same fixture |
| `<assign>` FMT | No icon-set conditional format | Open | P2 | No `IconSet` variant in `CfRule` (`crates/casual-calc-model/src/sheet.rs:899-979`); import records it Omitted/NotRetained (`crates/casual-calc-import/src/lib.rs:1391-1441`), and `:167-181` explains why it cannot simply be retained — the presentation lives in a `<dxf>` the writer regenerates by index. Note Sheets does not have this either, so it is two of three, not three of three | Author a three-arrow rule; assert it paints and that a re-import from the written file reproduces it |
| `<assign>` FMT | CF text, blank, error and date-period predicates are absent | Open | P2 | Only `TextContains` exists among text predicates; `beginsWith`, `endsWith`, `notContainsText`, `containsBlanks`, `containsErrors` and `timePeriod` appear in the importer only as report keys (`crates/casual-calc-import/src/lib.rs:187-198`). `timePeriod` is the exception and is handled well — kept as the equivalent formula and recorded Degraded (`:1397-1430`) | Author "is blank" and "begins with"; assert both paint and both survive a round trip |
| `<assign>` FMT | A CF rule cannot set a border or a number format | Open | P2 | `ConditionalFormat` carries fill, font colour, bold, priority and stop-if-true only; a dxf stating just a border or numFmt is counted and the rule dropped (`crates/casual-calc-import/src/lib.rs:1455-1470`) | Import a red-border-only rule; assert it paints and re-exports |
| `<assign>` SDK | No consumable Rust binding and no headless converter | Open | P2 | All sixteen crates are `publish = false`, `casual-calc-sdk` included (`crates/casual-calc-sdk/Cargo.toml:8`); the only workspace binary is the collaboration server. Every writer already exists behind the SDK — XLSX/ODS/CSV via `SessionFormat` (`crates/casual-calc-sdk/src/lib.rs:72-100`) and PDF via `casual-calc-render` — so this is packaging, and it repeats `IO-14`'s lesson at a larger scale | A CLI binary converts a fixture `.xlsx` to `.pdf` and `.csv` in CI, byte-identically across two runs |
| `<assign>` UX | No workbook-wide comment list | Open | P2 | `session_comments` is per-sheet and takes a rectangle (`crates/casual-calc-wasm/src/objects.rs:1366`); the only whole-document use is `selectCommentedCells` (`webapp/editor.core.js:7224-7238`), which is Go To Special ▸ Notes, not a list. There is no comments panel among the nine. Replies and resolve already exist (`objects.rs:254`, `:283`) | A panel lists every thread across every sheet, with resolved threads distinguishable and each entry navigating to its cell |
| `<assign>` UX | No @mentions, assignment or notification on comments | Open | P2 | The comment model is closed — `crates/casual-calc-model/src/sheet.rs:806-830` with `deny_unknown_fields` at `:812` — and carries no mention or assignee field. The collaboration server does know the roster (`server/casual-calc-collab-server/src/presence.rs:48-62`), so "who could I mention" is answerable; the notification itself belongs to the host | A mention round-trips through the file and raises a host callback |
| `<assign>` UX | No command palette | Open | P2 | Absent entirely; the only match in `webapp/*.js` is a comment at `webapp/editor.core.js:6144` naming "the command palette" among text fields the shell must release accelerators for. Every command already has an id (`:1218-1230`) and `listCommands`/`run` is already public SDK surface (`webapp/embed.d.ts`) | Typing "conditional" in the palette and pressing Enter runs the same command id the menu item runs, asserted against `listCommands()` |
| `<assign>` UX | Paste Special, row height and column width have no menu-bar route | Open | P3 | The Edit menu (`webapp/editor.core.js:9710-9738`) has no Paste Special entry; row height and column width exist only on the row/column header context menu (`webapp/editor.dialogs.js:2835`). All three dialogs are built and working — this is routing only | Each of the three opens from the menu bar and runs the same command id as its current route |
| `<assign>` UX | Zoom is clamped to 25–200% | Open | P3 | `export const ZOOM_MIN = 0.25, ZOOM_MAX = 2;` (`webapp/editor.core.js:1588`); Excel is 10–400%. The renderer already scales continuously | Zoom to 10% and 400% and assert the viewport layout still equals full layout restricted to the window |
| `<assign>` UX | No service worker and no web manifest | Open | P2 | No `sw.js`, no manifest and no `theme-color`/`apple-touch-icon`/`color-scheme` in `webapp/`; `webapp/editor.html` registers nothing. The engine is entirely client-side once loaded, so only a cold start fails. The head tags are the named remainder of `UX-MOB-06` | Load once online, go offline, reload — the editor starts and opens a local file |
| `<assign>` A11Y | No accessibility checker, no alt text, no `forced-colors` | Open | P2 | No alt-text field on charts or images (`grep alt_text\|altText\|descr` over `crates/casual-calc-model/src/chart.rs` and `webapp/editor.charts.js` → nothing); no rule-based a11y harness in `tests/browser` (the two tests there are targeted behavioural ones); no `forced-colors` block, though `prefers-contrast: more` is honoured (`webapp/editor.css:1886`, `:1896`). Split when scheduling: `forced-colors` is an afternoon, alt text is a model change, a checker is a project | Three gates: a `forced-colors` render test; alt text round-tripping on a chart; and a checker reporting a missing-alt-text chart and a merged header row |
| `<assign>` IO | WOPI cannot save-as or rename | Open | P2 | The client implements CheckFileInfo, GetFile, Lock, RefreshLock, Unlock and PutFile (`server/casual-calc-wopi/src/wopi.rs:175`–`:309`) and neither `PutRelativeFile` nor `RenameFile`; `webapp/editor.core.js:966` sets `canSaveAs: false` for the `wopi` preset. The bytes side is done — `session_save_as` and `writable_extensions` already produce a file under any supported name | A save-as against a WOPI mock creates a new file with the requested name and returns its id |
| `<assign>` DOC | `docs/53` no longer regenerates and under-reports the product | Open | P2 | `tools/feature-audit/inventory.py:20-21` reads `webapp/editor.js` — now a 17-line re-export shim — and regexes `pub fn session_*` out of `crates/casual-calc-wasm/src/lib.rs`, now a 4-export module root after the 16-module split. Running it prints `FNS=0/MUT=0/UNDO=0` for all 29 areas, and the committed summary table still shows Print setup and Charts at 0 while its own rows mark them ✅. `docs/14` states that `docs/53` "cannot drift in the way these did"; that is no longer true | `inventory.py` run in CI produces non-zero counts for a named area and fails if any area reports zero functions while its row is ✅ |
| `<assign>` DOC | Stale claims in `docs/12`, a code comment and a crate doc comment | Open | P2 | Extends `DOC-051`. `docs/12` §3.21/§8.11 say comments have no replies (false when written — `crates/casual-calc-wasm/src/objects.rs:254` landed 2026-08-22); §3.15 misstates which structured-reference forms fail; §3.8 and `docs/47` say validation does not reopen with its rule (`DV-04` closed it); §4.2 says the shortcut dialog lists eight rows (it lists fifteen, `webapp/editor.core.js:9573-9587`); §5 item 6 and `CLAUDE.md` say the SDK is `0.0.0` without type declarations (it is `0.0.1` with them). `webapp/editor.core.js:9668` says Share is hidden in every mode preset (`:955`/`:958` set it true). `crates/casual-calc-formula/src/lib.rs:14` lists whole-column ranges and structured references as deferred; both evaluate | `check-doc-references` extended to fail on a `docs/12` citation whose tracker row is `Done`, plus the specific corrections landed |
| `<assign>` PERF | Four of seven benchmark cases have no committed baseline, and none of them is an eval case | Open | P2 | `benchmarks/baselines/dev-reference.json` holds `model-snapshot-roundtrip-10k` and `package-open-small` only; the tool defines at least `display-list-frame-serialise`, `render-frame-1600x900`, `eval-incremental-edit-scaling`, `eval-kept-graph-edit-scaling` and `eval-kept-graph-range-edit-scaling` besides. The 1M / 60 fps / <50 ms figures in `CLAUDE.md` and `docs/30` are therefore design targets, not results, and are being quoted as results | A baseline on a named machine covering at least one eval-scaling case, with the regression gate wired to it |
| `<assign>` CALC | 25 modern functions absent, and `AGGREGATE` is described as present in prose | Open | P2 | Parsed from `FUNCTIONS` (`crates/casual-calc-eval/src/functions/mod.rs:59`): absent are the 2022 shaping family (VSTACK, HSTACK, TAKE, DROP, CHOOSECOLS/ROWS, TOCOL, TOROW, WRAPROWS/COLS, EXPAND), the text-split family, RANDARRAY, the 2024 GROUPBY/PIVOTBY/PERCENTOF/TRIMRANGE set, REGEX*, AGGREGATE and the FORECAST family. Spill semantics — the expensive substrate — already exist. Separately, `crates/casual-calc-sdk/src/lib.rs:2575` and `tests.rs:1078` read as if `AGGREGATE` ships | The bijection tests already gate catalogue/dispatch; the row's gate is a per-function test for each added name plus a doc fix asserting no prose names an uncatalogued function |
| `<assign>` DATA | Five filter refinements are carried and never evaluated | Open | P2 | `FilterRule::Unevaluated` (`crates/casual-calc-model/src/sheet.rs:429-443`) names `<top10>`, `<dynamicFilter>`, `<colorFilter>`, `<iconFilter>` and `<dateGroupItem>` and states "The rows it would hide are left visible". The retention is deliberate and right; the cost is that `SUBTOTAL` and the status-bar aggregates read differently from Excel with nothing on screen saying why | Either evaluate them (a per-rule visibility test against a fixture) or, as the cheap first step, a visible marker plus a test asserting it appears when a sheet carries an unevaluated rule |

**Rows deliberately not proposed.** C-4 (external data), P-4 (automation), P-6
(offline reconciliation), P-11 (portable history), P-13 (REST API), R-5 (shapes),
X-6 (worker thread) and X-12 (mobile app) are §8.2 decisions. Filing them as Open
rows would put seven items on the tracker that nobody can start, which is how
`docs/48` became a second roadmap nobody re-derived. They belong in a design
conversation with the owner first.

---

## 10. Reproducing this

**Read, in this order.** `docs/12` §1 (how to trust a measurement here), then
this document's §2.4, then `docs/14` and `docs/14a` for row status — never the
prose about a row. `docs/53` for feature correctness, **remembering that its
generator is broken** (§2.4). `docs/51` is a month stale and under-reports; read
it only for the constructs no other document covers.

**Run, and it is cheap.**

```sh
# The counts in §2.3
ls crates/ | wc -l
grep -c '#\[wasm_bindgen\]' crates/casual-calc-wasm/src/*.rs | awk -F: '{s+=$2} END {print s}'
wc -l webapp/*.js | tail -1
python3 -c "import re;print(len(re.findall(r'name:', open('crates/casual-calc-eval/src/functions/mod.rs').read())))"

# The absences asserted above
grep -rn extLst crates/casual-calc-import/src/ crates/casual-calc-export/src/
grep -rn iterat crates/casual-calc-wasm/src/
grep -rn "new Worker" webapp/ sdk/ desktop/
grep -rn "flash ?fill\|goal seek\|solver\|scenario manager" -i webapp/ crates/
grep '^publish' crates/*/Cargo.toml

# The two probes that produced C-1 and X-1
#   Build a throwaway crate OUTSIDE this tree depending on the repo crates by
#   path, and run the real evaluator. Carry a control in every run — the first
#   harness used store_formula where the repo uses store_formula_at with an
#   Origin (crates/casual-calc-eval/src/tests.rs:45-56), so every relative
#   reference was off by the cell's own address and SUM(A1:A3) came back #REF!.
#   That was one step from reporting whole-column references as broken.
```

**What needs a browser, and what it would change.** The following are the places
where running the editor would most likely reorder this document. `docs/73`'s
standing assumption applies: expect it to.

- **P-1** — open A named `X`, capture a version, reload, open B named `X`,
  capture, reload, reopen A. This is a `P0` derived statically from the keying,
  the `put` and the prefix delete. Reproduce before scheduling.
- **X-4** — whether F2-then-type genuinely works with a real IME, and whether a
  candidate window can commit into the `<textarea>`. Synthetic composition events
  are not an IME.
- **R-4** — put rotated, wrapped, underlined, vertically-centred text in a cell,
  export to PDF, open it. Two minutes, and it settles the sharpest claim in §2.4.
- **C-1** — whether the three failing forms produce the errors predicted here in
  the editor's own formula bar, and whether the range finder and autocomplete
  behave sanely around them.
- **Every figure in §2.3 marked "not re-measured"** — commands, toolbar controls,
  payload, cold boot. Four numbers, one session.
- **The whole of `docs/12` §3.1-§3.4, §3.6, §3.9-§3.11, §3.13, §3.16, §3.18,
  §3.20 and §4.3-§4.7** — left standing here and eleven days old.

**How the gap agents were briefed**, for anyone repeating the round: read-only,
one bucket each, `file:line` for every OpenCalc claim, a source and a date for
every competitor claim, `unverified` an acceptable answer and a confident wrong
finding not. Four surface agents produced the competitor inventories first from
published documentation only. The verification step that mattered was the
orchestrator opening the cited lines afterwards — that is what caught the four
wrong surface claims in §2.4, and it cost minutes.

---

## 11. Competitor sources

All fetched **2026-09-09** unless the page carries its own date. Where a claim
above is marked `unverified`, no source below covers it.

**Microsoft Excel.** Specifications and limits `1672b34d`; available chart types
`a6187218`; functions (alphabetical) `b3944572`; dynamic arrays `205c6b06`;
conditional formatting `fed60dfa`; data validation `29fecbcc`; structured
references `f5ed2452`; PivotTables `a9a84538`; number format codes `5026bbd6`;
Go To Special (find-and-select); paste options `8ea795b0`; paste special
`e03db6c7`; defined names `4d0f13ac`; outline/group `08ce98c4`; what-if analysis
`22bffa5f`; Analysis ToolPak `6c67ccf0`; Solver `5d1a388f`; Power Pivot
`f9001958`; Power Query `7104fbee`; co-authoring `7152aa8b`; Sheet Views;
protect a worksheet `3179efdb`; protection and security `be0b34db`; comments and
notes; accessibility checker `a16f6de0`; file formats `0943ff2c`; browser
differences `f0dc28ed`; unsupported features in the web app `7220105d`; language
accessory pack `82ee1236`; Python in Excel `a33fbcbe`; Office Scripts and Office
Add-ins on `learn.microsoft.com` (`ms.date` 2026-02-26 and 2026-06-17).

**Google Sheets.** Function list `table/25273`; Drive file limits
`drive/answer/37603`; keyboard shortcuts `181110`; conditional formatting
`78413`; data validation `186103`; sort and filter `3540681`; slicers `9245556`;
pivot tables `1272900`; tables `14239833`; table references `15637642`; charts
`63824` and `190718`; named functions `12504534`; LAMBDA `12508718`;
ARRAYFORMULA `71291`; QUERY `3093343`; IMAGE `3093333`; AI function `15877199`;
Smart Fill `9914525`; smart chips `12319513`; data cleanup `6325535`; number
formats `56470`; paste special `161768`; find and replace `62754`; named ranges
`63175`; freeze/group `9060449`; print `7663148`; import `40608`; Drive export
MIME types (`developers.google.com`); Excel-and-Sheets best practices `9331167`;
version history `190843`; offline `6388102`; protect ranges `1218656`; comments
`65129`; sharing `2494822`; Apps Script and quotas (`developers.google.com`);
macros `7665004`; Sheets API limits (`developers.google.com`); Connected Sheets
`9703214`; IMPORT functions `12188454`; screen readers `6282736`; locale
`58515`; images `9224754`; mobile `6000292`; Sheets canvas
(`blog.google`, published 2026-08-13). Password-protected `.xlsx` in Sheets is
sourced from long-running `support.google.com` threads (2019/2021/2023) and no
first-party page.

**ONLYOFFICE.** Spreadsheet editor user guides on `helpcenter.onlyoffice.com` —
topic index, Home tab, Data tab, insert function, insert chart, conditional
formatting, data validation, pivot tables and the Pivot Table tab, remove
duplicates, goal seek, solver, sheet view, version history, collaborative
editing, commenting, protect spreadsheet and protect sheet, save/print/download,
supported formats, keyboard shortcuts, screen reader, roadmap, JWT. Office API
and Docs API on `api.onlyoffice.com` — spreadsheet API class list,
`AddCustomFunction`, config/events/methods, Automation API, plugins, conversion
API and tables, Document Builder, WOPI FAQ, connectors. Blog: 9.4 (published
2026-05-19), 9.3 (2026-02-24), 9.1 (2025-10), plugin marketplace (2023-07),
VBA-to-JS (2023-07), accessibility (2026-04). `onlyoffice.com/compare-editions`,
`/all-connectors`, `/app-directory`, `/license-faq`, `/developer-edition`,
`/wopi-comparison`. GitHub: `ONLYOFFICE/{DocumentServer,sdkjs,web-apps,core}`
metadata and the `web-apps` recursive git tree (33,993 paths, not truncated);
`sdkjs/common/macro-recorder.js`; issues `DocumentServer#3576` and `#540`;
`community.onlyoffice.com` threads on Power Query, LET/TAKE and LAMBDA.
