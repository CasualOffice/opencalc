# 96 — Excel fit-gap: features and UX

**Status: analysis, 2026-10-14.** Owner, 2026-10-14: *"prepare a priority and fit gap
analysis with excel as well as UX inconsistencies … we need full tracker … to become a
competitor of excel … UX and polish of our product to be exactly same as excel."* Every
row below is in [14](14-EXECUTION-TRACKER.md) under "Excel parity backlog", with the
same id (`XL-FEAT-*`, `XL-UX-*`). Target design: [95](95-EXCEL-SHELL-TARGET.md).

## How this was produced, and how far to trust it

Two read-only audits, run in parallel. Neither trusted the earlier docs (92, 73, 47):
they re-checked every claim against the code and, where they could, **ran it**.

- **Features**: the wasm engine was loaded in Node and driven through the same
  `session_*` calls the editor makes.
- **UX**: the live editor at this branch was driven with Playwright, and screenshots
  were taken.

Rows resting on remembered Excel behaviour are marked **[unverified]**.

**Re-verified independently before filing.** The orchestrator re-ran each P0 claim
below rather than accept the report:

| Claim | Probe | Result |
| --- | --- | --- |
| XL-FEAT-002 SUMPRODUCT ignores computed arrays | `=SUMPRODUCT(--(A1:A5>2))`, `=SUMPRODUCT((A1:A5>2)*A1:A5)` over 1..5 | 0 and 0 (Excel: 3 and 12) |
| XL-FEAT-003 IF does not lift over arrays | `=SUM(IF(A1:A5>2,1,0))`, `=MAX(IF(A1:A5>2,A1:A5))` | 0 and 0 (Excel: 3 and 5) |
| XL-FEAT-004 unary minus on a range | `=SUM(-A1:A5)` | `#VALUE!` (Excel: −15) |
| XL-FEAT-001 typed input parsing | `12/31/2026`, `50%`, `$5.00`, `1,234.5`, `TRUE`, `(5)` | all stored as text |
| XL-UX-001 Show Formulas | Ctrl+` on the demo sheet | D2 `=#REF!*#REF!`, D5 `=SUM(#REF!:#REF!)` |
| XL-UX-002 Ctrl+− | one cell (B2) selected | row 2 deleted, no dialog |
| XL-UX-003 F2 in Point mode | `=SUM(` ↑ F2 ← ← | still inserting references (`=SUM(A9`) |
| XL-UX-004 Escape on ribbon popups | Fill Color open, Escape | stayed open — **fixed on this branch**, spec red first |

## Priorities

| | P0 | P1 | P2 | P3 | Total |
| --- | --- | --- | --- | --- | --- |
| Features (`XL-FEAT`) | 10 | 34 | 40 | 24 | 108 |
| UX (`XL-UX`) | 4 | 40 | 66 | 35 | 145 |

P0 means it blocks an Excel user from switching, loses data, gives a silently wrong
answer, or looks broken. P1 is a daily-use gap, P2 is polish or a weekly gap, and P3 is
rare or power-user.

**The order of work this implies.**

1. **Wrong answers and data loss first.** These are XL-FEAT-001 to 010 and XL-UX-001 to 003.
   A user who meets one of these leaves, and is right to.
2. **Daily Excel muscle memory next.** These are the P1 UX rows: selection semantics, Excel's
   dialogs, the destructive shortcuts, and formula AutoComplete.
3. **Then the missing feature families**: images and shapes, chart types, What-If, and the
   missing functions.
4. **Beyond Excel** (added 2026-10-15, owner): agent access (docs/97) and the items from the
   external review below, worked in the same tracker and the same way — a spec seen red,
   our UX, no silent data loss.

## Owner decisions these rows need

- ~~Icon-only ribbon versus Excel's labelled ribbon~~ — **decided 2026-10-14**: group names
  on, button labels off (docs/95 rule 1); XL-UX-006/007/010 done, 093 done.
- **Panels versus Excel's dialogs.** Conditional Formatting, Data Validation, Page Setup and
  Find & Replace are side panels or bars here, and dialogs in Excel (XL-UX rows).

## Feature parity by area

| Area | Parity | One-line reason |
|---|---|---|
| Cell entry & editing | **Partial (broken core)** | Only ISO dates and plain `f64` parse. `12/31/2026`, `1,234.5`, `50%`, `$5`, `1:30 PM`, `TRUE` and `(5)` all store as **text** (probe p1). No IME start-typing. |
| Formula language / parser | **Partial** | A1, `$`, sheet refs, `A:A`, structured refs, LAMBDA calls, omitted args. No array constants, `A1#`, `@`, whole-row `1:1`, 3-D, union, intersection, R1C1 or external refs (probe p2, p8). |
| Function library | **Partial (~81%)** | 429 functions out of a ~526-name Excel list. 98 missing, incl. XOR, AGGREGATE, CEILING.MATH, TEXTSPLIT, VSTACK, TAKE, REGEX*, GROUPBY. |
| Dynamic arrays / LAMBDA | **Partial** | Spill and `#SPILL!` work. LET, LAMBDA, MAP, REDUCE, SCAN, BYROW, BYCOL, MAKEARRAY, FILTER, SORT, SORTBY, UNIQUE, SEQUENCE, XLOOKUP and XMATCH all evaluate. But SUMPRODUCT, IF and unary operators break on arrays, and the spill-ref operator `A1#` cannot be written. |
| Calculation engine | **Partial** | Incremental recalc, manual/auto mode, cancellable. Iterative calc exists in the model with no way to turn it on. Single-threaded. |
| Number formats | **Partial** | Custom codes, accounting, bracketed colours, elapsed time. **Fractions render wrong.** 14 presets, no locale pickers. |
| Conditional formatting | **Partial** | Cell value (gt, lt, eq, between), text contains, top/bottom, above/below average, duplicate/unique, colour scale, data bar, formula. No icon sets. ≥ ≤ ≠ and not-between exist in the model but cannot be authored. Format is fill-only. |
| Data validation | **Full-ish** | List (literal or range), whole, decimal, date, time, length and custom. Input/error messages and styles. Existing rule reloads. No circle-invalid. |
| Tables | **Partial** | Create, style, banding, totals row, autoexpand, rename. `[@Col]`/`[#This Row]` are broken (P0). No slicers. |
| Sort & filter | **Partial** | Sort capped at 3 keys, values only. AutoFilter supports values plus two custom comparisons. Colour, top-10 and date filters are carried but not evaluated. No Advanced Filter. Personal filter views are a lead. |
| PivotTables | **Partial** | Create, refresh, 11 aggregates, sort, filter. Show Values As, calculated fields and grouping are in the model but cannot be reached. No slicers, timelines or pivot charts. Pivots export as cells (`PIV-02`). |
| Charts | **Partial** | 7 types, no subtypes in the UI. No combo/secondary/stacked controls, trendlines, error bars, per-series colour or axis formatting. No sparklines. |
| Images / shapes / SmartArt | **Missing** | Imported pictures are drawn read-only. Nothing can be inserted. No shape or text-box model. |
| Comments / notes | **Partial** | Threaded comments import/export, reply and resolve. No @mentions, no comments pane, no notes-vs-comments distinction in the UI. |
| Find & replace / Go To | **Partial** | Find supports match case, whole cell, values, all sheets and wildcards. Replace works per sheet. Go To Special only offers notes. |
| Names | **Partial** | Define, delete, go to. No edit or rename, and no sheet scope from the UI. |
| What-if (Goal Seek / Data Table / Scenarios / Solver) | **Missing** | Nothing. |
| Get & Transform / connections / data model | **Missing** | Nothing, by design (no network layer). |
| Data types / Flash Fill / Copilot / Python | **Missing** | Nothing. |
| AutoFill | **Partial** | Numbers, linear/growth series, days, months, dates and "Item n". `Q1` fills to Q5 (Excel wraps to Q1). No custom lists. |
| Outline / subtotals | **Partial** | Manual group, ungroup and levels are good. No Subtotal command, no Auto Outline. |
| Freeze / split / windows | **Partial** | Freeze is full. No Split, no New Window or Arrange, no Custom Views. Zoom is 25–200%. |
| Protection & encryption | **Partial / Missing** | Lock and hide-formula plus sheet protect, with no password. A file's allowances are honoured on read. No workbook-structure protection, no Allow Edit Ranges, no file encryption. An encrypted file is reported as corrupt. |
| Page layout & printing | **Partial** | Print area, title rows, margins, orientation, fit-to, header/footer text, manual breaks, PDF export. No Page Break Preview or Page Layout view. HTML print drops charts, images and CF. PDF flattens rotation, wrap and similar. |
| File formats | **Partial** | xlsx/xlsm round-trip byte-identically when unedited. Edited saves lose `extLst` and unparseable formulas. ODS is values and formulas only (losses counted). CSV, TSV and PSV supported. No xls, xlsb, xltx or xlam. |
| Macros / VBA / Office Scripts / add-ins | **Missing** | Macros are retained and never run. No scripting or custom functions. |
| Collaboration / version history | **Partial (lead in places)** | OT co-editing, presence, relay, resume, per-op ACLs, personal filter views. `COL-50` divergence is still open. Version history is local IndexedDB only and has a P0 name-collision bug. No offline merge. |
| Accessibility / spelling | **Partial / Missing** | A real ARIA grid over the canvas (a lead). No Accessibility Checker, no alt text, no spell check. |
| Internationalisation | **Missing** | English only. No `;` argument separator, no localised function names, no RTL, no IME start-typing. |
| Keyboard coverage | **Partial** | Large chord set with a parity suite, Alt+Q command search and F4 anchor cycling. Ctrl+E (Flash Fill) does nothing. (UX agent owns the detail.) |
| Hyperlinks | **Full** | `session_set_hyperlink`, HYPERLINK(), Ctrl+K. |
| External links | **Missing (lossy)** | `[1]Sheet1!A1` does not parse. The value is frozen and the formula is lost on save. No Edit Links. |
| Form controls / in-cell checkbox | **Missing** | Nothing. |
| Performance / limits | **Partial** | Grid is 1,048,576 × 16,384 (`crates/casual-calc-formula/src/reference.rs:43-45`). Node probe: 300k-cell paste 4.1 s, 100k-dependent fan-out 184 ms, save 415 ms on the main thread, no Worker. Only 2 micro baselines are committed. |

## Biggest UX differences

1. **Show Formulas (Ctrl+`) is broken.** Every relative formula renders as `#REF!` (`=SUM(#REF!:#REF!)`, `F!*#REF!`). The grid prints the stored expression with no origin cell.
2. **Destructive shortcuts skip Excel's dialogs.** Ctrl+- with one cell selected deletes the **entire row**. Ctrl+Shift+= inserts a whole row. Excel asks "Shift cells up/left, Entire row/column".
3. **Formula editing does not follow Excel's modes.** F2 does not toggle Point→Edit, and arrow keys after F2 keep inserting references in the middle of the formula, which corrupts it (`=SUBSTITUTE(A3$A$5`). There is no cell AutoComplete, and the in-cell editor, formula bar and name box use a monospace font.
4. **The ribbon is icon-only.** It has no group names, no large-button labels, no dialog launchers, no ScreenTips (native `title` only) and single-level KeyTips. Folded groups at 1024/768 px are bare icons. **[owner decision]**
5. **Many core Excel dialogs are missing or replaced.** Go To / Go To Special, Name Manager, New Name and Protect Sheet are missing. Find & Replace, Conditional Formatting, Data Validation and Page Setup are replaced by bars, popovers or side panels. Insert Function has no categories, descriptions or Function Arguments step.
6. **Escape does not close ribbon popups** (Orientation, Fill Color, and similar). This breaks docs/95 rule 5.
7. **Selection semantics differ.** Shift+click and Ctrl+A move the active cell. Enter leaves a multi-cell selection instead of cycling inside it. The name box keeps showing `3R x 2C`. The ContextMenu key moves the selection to wherever the mouse is.
8. **The status bar and sheet-tab row are Google-Sheets-like.** They share one row, show "Local only · nothing uploaded", leave stale debug-ish messages ("ok", "resized", the clipboard-refused sentence), and have no customise menu, no view buttons, no horizontal scrollbar, and the + button sits before the tabs.
9. **Ribbon content is a thin subset.** Whole Excel groups are absent: Page Layout page-setup controls, Formulas function library, Review proofing/comments, View workbook views. Non-Excel commands sit where Excel has none (Column Statistics, Convert Text to Numbers, Zero Values, Version History on View, Print on Page Layout).
10. **Wording and casing are inconsistent.** Menus and half the dialogs use sentence case ("Paste special", "Insert function", "Find & replace…", "Fill down") against Excel's Title Case. Context menus differ in order and items. Row height and column width are in **px**, not points/characters.

## Feature gaps (`XL-FEAT`)

| ID | Area | Gap | Excel behaviour | OpenCalc today (evidence) | Pri | Effort | Notes |
|---|---|---|---|---|---|---|---|
| XL-FEAT-001 | Cell entry | Typed dates, times, percents, currency, thousands separators, booleans and accounting negatives become **text** | `12/31/2026`→date, `1,234.5`→number, `50%`→0.5 with % format, `$5.00`→5 currency, `1:30 PM`→time, `TRUE`→boolean, `(5)`→-5 | `input_edit` accepts only ISO dates (`parse_iso_datetime`) then `trimmed.parse::<f64>()`. Everything else is interned as a string (`crates/casual-calc-sdk/src/lib.rs:1102-1218`, ISO branch ~1163, f64 ~1194). **Probe p1**: all of `12/31/2026, 31/12/2026, 31-Dec-2026, Dec 31 2026, 1,234.5, 50%, $5.00, 1:30 PM, TRUE, 1/2, (5), €5` stored left-aligned as text. Only `13:45`, ISO dates and `1e3` became numbers. The editor does no pre-parse (`webapp/editor.selection.js:580`). | P0 | M | Doc 92 X-1, still open. Wider than recorded: **typed `TRUE` is text**, and `%`/`$` are text too. First minute for every user. Fix belongs in `input_edit` with a locale policy. |
| XL-FEAT-002 | Functions / arrays | `SUMPRODUCT` ignores computed array arguments, so conditional SUMPRODUCT returns 0 | `=SUMPRODUCT(--(A1:A5>2))`=3, `=SUMPRODUCT((C1:C5="a")*B1:B5)`=sum | **Probe p4**: `SUMPRODUCT(--(A1:A5>2))`→0, `SUMPRODUCT((A1:A5>2)*(B1:B5))`→0, `SUMPRODUCT(A1:A5*1)`→1, `SUMPRODUCT(SEQUENCE(3))`→1, `SUMPRODUCT(--(A1:A5>2),B1:B5)`→#VALUE!. `SUM(A1:A5*1)`=15 is correct, so the fault is `flatten_values` in `eval_sumproduct` (`crates/casual-calc-eval/src/functions/math.rs:130-156`). | P0 | S | **New, not tracked.** Plausible wrong number with no marker, in the most common legacy multi-criteria idiom. It hits imported workbooks with no user action. |
| XL-FEAT-003 | Functions / arrays | `IF` does not evaluate element-wise over arrays (CSE / dynamic-array IF) | `=SUM(IF(A1:A5>2,1,0))`=3, `=MAX(IF(C1:C5="a",B1:B5))`=max of matches, `=IF(A1:A5>2,"y","n")` spills | **Probe p4**: →0, →0, and a single `n` with no spill. `eval_if` coerces arg 0 with `as_bool()` and branches once (`crates/casual-calc-eval/src/functions/mod.rs:1232-1247`). | P0 | M | **New.** Every legacy array formula of the form `{=SUM(IF(...))}` and every modern `MAX(IF(...))` gives a wrong number silently. Check IFS, IFERROR, IFNA, CHOOSE and SWITCH for the same shape. |
| XL-FEAT-004 | Formula language | Unary `-`/`+`/`%` on a range or array gives `#VALUE!` | `=-(A1:A5)` spills negatives. `--(range>2)` coerces booleans to 1/0. | **Probe p3/p4**: `=-(A1:A5)`→#VALUE!. `eval_unary` calls `as_number()` on the whole value (`crates/casual-calc-eval/src/eval.rs:575-586`). Binary ops do broadcast (`:593`). | P0 | S | **New.** This is the root of the `--` failures in 002. Fix by lifting through `broadcast_binary`'s machinery. |
| XL-FEAT-005 | File fidelity / functions | Opening a file that uses an unimplemented function replaces Excel's cached value with `#NAME?`, writes `#NAME?` back, and drops the `_xlfn.` prefix | Excel shows the computed value | `call_function` falls to `_ => Value::Error(ErrorValue::Name)` (`crates/casual-calc-eval/src/functions/mod.rs:1200`). Open runs `recalculate` (`crates/casual-calc-sdk/src/lib.rs:684-686`). **Probe p6**: a file with `XOR(TRUE,FALSE)` cached 1 and `_xlfn.CEILING.MATH(4.2)` cached 5 showed `#NAME?`. The saved sheet has `<c t="e"><f>XOR(TRUE,FALSE)</f><v>#NAME?</v>` and `<f>CEILING.MATH(4.2)</f>` with the **prefix stripped**, which Excel itself reads as `#NAME?`. `session_import_summary()` and `session_save_loss()` were both empty. | P0 | M | **New.** Breaches the no-silent-loss rule. Cheap mitigation: treat an unknown function as "keep cached value, mark stale, report", and re-emit `_xlfn.` for every name not in the catalog. The real fix is 006 + 012/013. |
| XL-FEAT-006 | File fidelity / parser | Formulas the parser cannot read are **converted to constants** on the first edited save | Round-trips every formula | Import keeps the cached value and drops the expression (`crates/casual-calc-import/src/lib.rs:1130-1133`, `Degraded/NotRetained`). **Probe p9**: `SUM({1,2,3})`, `SUM(1:1)`, `[1]Sheet1!A1`, `SUM(Sheet1:Sheet1!A1)` → saved as bare `<v>`. The import summary said only `partly read: f`, and `session_save_loss()` was empty. | P0 | S (retain text) / L (parse) | **New as a loss path.** Counted at import as an opaque `f`, not named per construct and not repeated at save. Retaining `<f>` text verbatim (as `Expr::Raw` already does for defined names, `crates/casual-calc-formula/src/ast.rs:113`) closes the loss. Parser work is rows 014–017. |
| XL-FEAT-007 | File fidelity | Worksheet `<extLst>` (sparklines, x14 CF/DV, slicer refs) silently dropped after the first edit | Preserved | `extLst` appears only in `crates/casual-calc-import/src/tests.rs`. Nothing in the model, import or export handles it. **Probe p6**: injected an `x14:sparklineGroups` ext, edited one cell, saved. 0 occurrences in the output and `session_save_loss()` was empty. | P0 | M | Doc 92 R-1, still open, now **observed**. Carry it verbatim in `Sheet::carried` as `<protectedRanges>` is. |
| XL-FEAT-008 | Tables | Current-row structured refs give errors or a **wrong total** | `[@Amount]` = this row. `[[#Data],[Amount]]` = column. `[#This Row]` = row. | **Probe p5**: `=Sales[@Amount]*2`→#VALUE!, `=[@Amount]*2`→#VALUE!, `=SUM(Sales[[#Data],[Amount]])`→#REF!, `=SUM(Sales[#This Row])`→100 (the whole table). `_ => (first_data, last_data)` (`crates/casual-calc-eval/src/eval.rs:217-220`) and the column lookup by raw spec (`:224-233`). | P0 | S | Doc 92 C-1, still open. Calculated columns in imported tables open as errors. |
| XL-FEAT-009 | Version history | Two workbooks with the same display name share a history, and saving one deletes the other's snapshots | History bound to file identity | `docKey()` returns `documentName()` (`webapp/editor.versions.js:76-78`). `store.put({key: doc, versions})` replaces the row and a prefix sweep deletes unlisted bytes (`:136-150`). `forgetVersions` is only called from File ▸ New (`webapp/editor.core.js:9723`). | P0 | S | Doc 92 P-1, still open. Data loss inside the safety feature. |
| XL-FEAT-010 | File fidelity / security | A digitally signed workbook is saved with its **stale** signature over changed content | Excel removes the signature on edit and says so | No `xmlsignatures` handling anywhere in `crates/` (grep). Reachable parts are retained and re-emitted verbatim (doc 92 §2.4: `crates/casual-calc-import/src/lib.rs:269-274`, `crates/casual-calc-model/src/workbook.rs:31`). | P0 | S | `SIGN-01` (Open P2) under-prices it. A broken signature reads as tampering. |
| XL-FEAT-011 | Calc engine | 1,000s of Excel workbooks rely on `0.1+0.2=0.3`-style comparisons being TRUE | Excel compares with 15-significant-digit tolerance, so `=0.1+0.2=0.3` → TRUE | **Probe p3**: → FALSE | P1 | S | Excel's "close to zero" snapping on `=`, and the final-ops rounding behaviour. Affects IF/COUNTIF results in money models. Excel's exact rule is [unverified] in detail. |
| XL-FEAT-012 | Functions | Missing common statistical, math and logic functions | Present | Not in `FUNCTIONS` (`crates/casual-calc-eval/src/functions/mod.rs:59-621`): **XOR, AGGREGATE, CEILING.MATH, FLOOR.MATH, CEILING.PRECISE, FLOOR.PRECISE, FORMULATEXT, T.DIST, T.DIST.2T, T.DIST.RT, T.INV, F.DIST, F.INV, CHISQ.DIST, CHISQ.INV, BETA.DIST, BINOM.INV, HYPGEOM.DIST, NEGBINOM.DIST, PERCENTILE.EXC, QUARTILE.EXC, PERCENTRANK.EXC, RANK.AVG, COVARIANCE.S, MODE.MULT, SKEW.P, CONFIDENCE.T, FORECAST.LINEAR, GAMMA, GAMMALN.PRECISE, GAUSS, PHI, ERF.PRECISE, ERFC.PRECISE, BASE, DECIMAL, ARABIC, ACOT, ACOTH, MUNIT, BINOM.DIST.RANGE, IMCOT/IMCSC/IMCSCH/IMSEC/IMSECH**. Probe p2: XOR, AGGREGATE, CEILING.MATH, FORMULATEXT and T.DIST all → `#NAME?`. | P1 | M | Mostly closed-form. XOR, AGGREGATE, CEILING.MATH and FORMULATEXT are the high-reach ones. Combined with 005, every one of them is a data-integrity issue on open. |
| XL-FEAT-013 | Functions / dynamic arrays | Missing 2022 array-shaping and text-splitting family | Present since 2022 | Missing: **TEXTSPLIT, TEXTBEFORE, TEXTAFTER, VSTACK, HSTACK, TAKE, DROP, CHOOSECOLS, CHOOSEROWS, TOCOL, TOROW, WRAPROWS, WRAPCOLS, EXPAND, RANDARRAY, ARRAYTOTEXT, VALUETOTEXT**. Probe p2: TEXTSPLIT, VSTACK, TAKE, RANDARRAY → `#NAME?`. | P1 | M | The spill infrastructure exists (`crates/casual-calc-eval/src/lib.rs` spill handling). These are mostly pure array functions. |
| XL-FEAT-014 | Formula language | Array constants `{1,2,3}` / `{1;2}` cannot be written or read | Core syntax | Lexer has no `{` token. **Probe p2**: `={1,2,3}` → `[OC-FML-0001] unexpected character '{'`. Imported ones become constants (006). | P1 | M | Very common in `SUM(COUNTIF(r,{"a","b"}))`, `LOOKUP`, `CHOOSE({1,2},…)`. |
| XL-FEAT-015 | Formula language | Spill-range operator `A1#` and implicit intersection `@` not parseable | `=SUM(E1#)`, `=@A1:A5` | **Probe p2**: `unexpected character '#'` and `unexpected character '@'`. `lex_error` treats `#` as an error-literal start only (`crates/casual-calc-formula/src/lex.rs:194-198, 260-272`). | P1 | M | `A1#` is how dynamic-array workbooks chain spills. Excel also writes `@` into files opened from pre-DA versions. Both are lost on save (006). |
| XL-FEAT-016 | Formula language | Whole-row references (`1:1`, `$1:$3`) | `=SUM(1:1)` | **Probe p2/p8**: `unexpected token: Colon`. `Sheet1!$1:$1` (print titles) is kept only as `Expr::Raw` (`crates/casual-calc-formula/src/ast.rs:103-113`). `A:A` works. | P1 | S | Asymmetric with the working `A:A`. |
| XL-FEAT-017 | Formula language | 3-D references (`Sheet1:Sheet3!A1`) | `=SUM(Jan:Dec!B5)` is a standard consolidation pattern | **Probe p2**: `unexpected token: Bang`. Crate doc lists it as deferred (`crates/casual-calc-formula/src/lib.rs:14`). | P1 | M | Monthly-tab budgets use this everywhere. |
| XL-FEAT-018 | Functions | `SORT` with an omitted `sort_index` gives `#VALUE!` | `=SORT(B1:B5,,-1)` sorts descending by column 1 | **Probe p3**: `#VALUE!`. `SORT(B1:B5,1,-1)` works. `Expr::Empty` is not defaulted in SORT. | P1 | S | **New.** Omitted-argument forms are common in recorded or AI-written formulas. Audit the other functions for `Expr::Empty` defaults. |
| XL-FEAT-019 | Functions / entry | Text-to-number coercion ignores separators and non-ISO dates | `=DATEVALUE("12/31/2026")` → serial, `=VALUE("1,234")` → 1234 | **Probe p2**: both → `#VALUE!` | P1 | S | Same root as 001. Fix the parser once and share it. |
| XL-FEAT-020 | CSV import | CSV fields typed with the same ISO-only rule. No import wizard (column types, locale, skip rows) | Locale-aware typing, plus Power Query / Text Import Wizard | **Probe p10**: `12/31/2026`, `"1,234"`, `50%`, `$5` from a CSV all stored as text. `session_open_delimited(bytes, delimiter)` takes the delimiter only (the generated wasm type declarations). | P1 | M | A US CSV of transactions opens as an all-text sheet. |
| XL-FEAT-021 | Number formats | Fraction formats render wrong | `TEXT(0.5,"# ?/?")` → `1/2`. Built-in numFmt 12/13. | **Probe p2**: → `"1  / "`. `crates/casual-calc-layout/src/numfmt.rs:14` "Deferred: fractions". | P1 | M | Wrong displayed value on files nobody customised. Doc 92 rated this P2. I raise it because the output is wrong, not missing. |
| XL-FEAT-022 | Internationalisation | Non-English UI, `;` argument separator, localised function names | 100+ UI languages, locale separators | `messages` is an empty Map (`webapp/editor.core.js:4904`). `availableLocales()` → en-US only (`webapp/editor.i18n.js:83-85`). The lexer has only `Token::Comma` (doc 92 X-2). | P1 | L | Excludes every non-English team. |
| XL-FEAT-023 | Cell entry | IME: CJK users cannot start typing into a selected cell | Composition starts in-cell | Printable branch `if (e.key.length === 1 && !mod) startInline(...)` (`webapp/editor.core.js:8716`). No `isComposing`/`compositionstart`/`229` anywhere in `webapp/*.js`. | P1 | M | F2 then type works. |
| XL-FEAT-024 | Internationalisation | Right-to-left sheets and text direction | Sheet RTL plus per-cell reading order | Doc 92 X-3: `direction: ltr` pinned in `webapp/editor.css`. `readingOrder` round-trips without rendering. [not re-opened this pass] | P1 | L | |
| XL-FEAT-025 | Images | Insert, move, resize and delete pictures. Image in cell. `IMAGE()` | Insert ▸ Pictures, Place in Cell, `IMAGE()` | Read-only: `session_images` (`crates/casual-calc-wasm/src/view.rs:416`) and `session_image_data` (`crates/casual-calc-wasm/src/objects.rs:1505`). No add binding. Ribbon comment says Pictures is deliberately omitted (`webapp/ribbon.model.js:7-10`). `IMAGE` → `#NAME?` (probe p2). | P1 | L | Day one for invoices and reports. |
| XL-FEAT-026 | Shapes | Shapes, text boxes, arrows, callouts, icons, SmartArt, WordArt | Full drawing layer | No Shape/TextBox type in `crates/*/src` (grep). Not in the ribbon (`webapp/ribbon.model.js:8`). Imported shapes are not drawn; whether they survive a chart edit in the same drawing is [unverified]. | P1 | XL | |
| XL-FEAT-027 | Charts | Chart catalogue limited to 7 types | ~17 families / ~50 subtypes: radar, stock, surface, bubble, histogram, Pareto, box & whisker, waterfall, funnel, treemap, sunburst, map, combo | `ChartKind` = Bar, Column, Line, Area, Pie, Doughnut, Scatter, Unsupported (`crates/casual-calc-model/src/chart.rs:35-53`). `CHART_KINDS` (`webapp/editor.core.js:4405-4408`). Imported others → `Unsupported` (`crates/casual-calc-import/src/chart.rs:567`). | P1 | L | `chartex` (2016 types) has no reader at all (grep). |
| XL-FEAT-028 | Charts | Stacked, 100% stacked, combo, secondary axis and data labels cannot be authored | Chart Design ▸ Change Chart Type / Format pane | The model carries `kind`, `secondary_axis`, `data_labels` per series (`crates/casual-calc-model/src/chart.rs:158-178`) and the wire accepts them. Panel offers only Type / Legend / Series / Category labels (`webapp/editor.core.js:4419-4560`). No "stacked" control in `webapp/*.js` (grep). | P1 | S | Doc 92 R-3. Engine-capable, so this is cheap. |
| XL-FEAT-029 | Charts | Chart formatting: series colours, axis min/max/units/log, number formats, gridlines, markers, fonts. Imported styling kept on edit. | Full Format pane | `ChartSeries` has no colour field (`crates/casual-calc-model/src/chart.rs:129-178`). Editing an imported chart replaces its definition and loses unmodelled formatting (the panel says so: `webapp/editor.core.js:4422-4426`). | P1 | L | |
| XL-FEAT-030 | PivotTables | Show Values As, calculated fields and date/number grouping cannot be reached | 15 Show-As modes, calc fields/items, grouping | The model has `PivotShowAs` (9), `PivotCalculatedField`, `PivotGroupBy` (`crates/casual-calc-model/src/pivot.rs:152-164, 277-288, 389`). Panel exposes aggregates and sort only (`webapp/editor.pivot.js:112-118`). `PIV-07` Partial: export writes no `showDataAs`/`fieldGroup`/`@formula`. | P1 | L | "% of total" is the first thing reached for after Sum. |
| XL-FEAT-031 | PivotTables | A pivot created here exports as plain cells, not a live pivot | Live PivotCache + PivotTable parts | `PIV-02` Partial (docs/14 row). | P1 | L | The recipient in Excel cannot refresh or pivot it. |
| XL-FEAT-032 | What-if | Goal Seek | Data ▸ What-If ▸ Goal Seek | Nothing (grep `goal.seek` in `webapp/` and `crates/` → none). The Data tab has no What-If group (`webapp/ribbon.model.js` Data tab). | P1 | M | A bracketed root finder exists for IRR (`crates/casual-calc-eval/src/functions/financial.rs`). |
| XL-FEAT-033 | Protection | Sheet protection password, allowance checklist, Protect Workbook (structure), Allow Edit Ranges | All present | Sheet toggle with no password (`webapp/editor.dialogs.js:2488`, `webapp/editor.sheets.js:363`). Allowances honoured on read only (`crates/casual-calc-model/src/sheet.rs:330` `permits`). No workbook-structure binding (exports list). | P1 | M | The model already stores an untyped allowance map. |
| XL-FEAT-034 | Encryption | Open or save a password-encrypted workbook | File ▸ Info ▸ Encrypt with Password | **Probe p10**: a CFB-magic file → `[OC-PKG-0005] not a valid ZIP/OPC package`. The error tells users their intact file is corrupt. No ECMA-376 Agile encryption handling (grep `EncryptedPackage` → none). | P1 | L (decrypt) / S (diagnose) | Split it: a "password-protected" diagnostic is hours of work. |
| XL-FEAT-035 | Legacy formats | `.xls` (BIFF8) and `.xlsb` open/save | Native | **Probe p10**: `.xls is not a format this build can open`, same for `.xlsb`. `openable_extensions()` = xlsx, xlsm, ods, csv, tsv, tab, psv. | P1 | XL | `IO-04` Partial P1. xlsb is common for large finance models. Templates (`.xltx`/`.xltm`) and `.xlam` are also refused. |
| XL-FEAT-036 | Printing | Page Break Preview and Page Layout view | Drag automatic and manual breaks; WYSIWYG page view | No view mode (grep). Manual breaks drawn. `session_page_breaks` exists. Automatic breaks live in `print::Plan` only (doc 92 R-6). | P1 | M | |
| XL-FEAT-037 | Printing | Print and PDF output do not match the sheet | WYSIWYG | Doc 92 R-4: HTML print drops charts, images and all CF (`crates/casual-calc-wasm/src/objects.rs` ~1820). PDF refuses images (`crates/casual-calc-render/src/pdf.rs:883`) and `PaintItem::Text` has no rotation, wrap or valign fields. [line numbers not re-opened, except pdf.rs:883] | P1 | L | For many users the printout is the deliverable. |
| XL-FEAT-038 | Automation | No macros, VBA editor, recorder, Office Scripts, custom JS functions or add-ins | VBA + Office Scripts + Office.js + COM | `.xlsm` macros retained, never executed (`crates/casual-calc-sdk/src/lib.rs:77-88` per doc 92). No script, eval or plugin export among the 293 wasm exports. Ribbon has no Developer or Automate tab. | P1 | XL | Needs a decision first. The cheapest credible step is a command recorder over the registry ids plus custom functions (LAMBDA names already work). |
| XL-FEAT-039 | Collaboration | Co-editing correctness: an insert concurrent with a delete diverges for formula ranges | Converges (server-ordered) | `COL-50` Open P1. `canShare` is off in embedded/host modes. | P1 | L | Semantics decision pending. |
| XL-FEAT-040 | Collaboration | Offline edits lost on unresumed reconnect | Offline edit and reconcile | Doc 92 P-6: `webapp/collab.js` announces the loss and discards. | P1 | XL | |
| XL-FEAT-041 | Data tools | Text to Columns: naive split, no quotes, fixed width, column types or overwrite check | Wizard with qualifier, fixed width, per-column type, destination | `text.split(delim)` in JS then `session_set_cell` per part (`webapp/editor.dialogs.js:2353-2356`). No engine binding. | P1 | M | Quoted CSV fields shred, and adjacent columns are overwritten silently. |
| XL-FEAT-042 | Performance | Save blocks the main thread. No worker. No committed eval benchmarks. | Native, multi-threaded calc, background save | **Probe p11** (Node, indicative only): 300k-cell paste 4.1 s; edit with 100k dependents 184 ms; `session_save` 415 ms; open 670 ms. No `new Worker` in `webapp/`. `benchmarks/baselines/dev-reference.json` holds 2 micro cases and no eval case. | P1 | L | The <50 ms recalc target is unmeasured in CI. Fan-out recalc is already 3.7× over it at 100k. |
| XL-FEAT-043 | Conditional formatting | Icon sets | 20 icon sets | No `IconSet` in `CfRule` (`crates/casual-calc-model/src/sheet.rs:895-990`). Import records it as omitted (`crates/casual-calc-import/src/lib.rs:188`). | P1 | M | Traffic-light dashboards are ubiquitous. Losses are counted. |
| XL-FEAT-044 | Desktop | Desktop app ships unsigned and macOS reports it as "damaged" | Signed | `"signingIdentity": "-"` (`desktop/tauri.conf.json:101`). `TAURI-008` Open P1 (a decision). | P1 | S | Not a spreadsheet feature, but it blocks install. |
| XL-FEAT-045 | Comments | @mentions and assignment | @mention notifies; assign task | No mention or assignee field. The comment model is closed (`crates/casual-calc-model/src/sheet.rs:806-866`). | P2 | M | |
| XL-FEAT-046 | Comments | Comments pane (all threads, filter, resolve-all). Notes vs Comments as two features. | Review ▸ Show Comments; separate Notes | Ribbon has "Note" only (`webapp/ribbon.model.js` Insert/Review). The thread UI is per cell (`webapp/editor.core.js:5609-5619`). `session_comments` is a rectangle query. | P2 | S | Threaded import/export does exist (`crates/casual-calc-import/src/read.rs:424`, `crates/casual-calc-export/src/lib.rs:393`). |
| XL-FEAT-047 | Sort | More than 3 sort keys, sort by colour or icon, custom-list order, left-to-right sort | 64 levels, colour, icon, custom lists, by row | `[0, 1, 2].forEach(addKeyRow)` (`webapp/editor.dialogs.js:783`). `session_sort_range_multi` takes keys and directions only (`crates/casual-calc-wasm/src/structural.rs:415`). | P2 | S (keys) / M (colour, list) | |
| XL-FEAT-048 | Filter | Filter by colour, top 10, dynamic dates ("this month"), icon filter | Present | Carried and **not evaluated**: `FilterRule::Unevaluated` leaves rows visible (`crates/casual-calc-model/src/sheet.rs:433-437, 601`). | P2 | M | SUBTOTAL then differs from Excel with no on-screen cue. |
| XL-FEAT-049 | Filter | Advanced Filter (criteria range, copy to, unique only) | Data ▸ Advanced | Absent (grep). | P2 | M | |
| XL-FEAT-050 | Conditional formatting | ≥, ≤, ≠ and not-between rules cannot be authored | Present | In the model (`crates/casual-calc-model/src/sheet.rs:910-918`) but `cf_rule_from_kind` maps only gt/lt/eq/between/contains/… (`crates/casual-calc-wasm/src/data.rs` ~465-520). | P2 | S | Doc 92 R-8. A file can carry a rule the UI cannot author. |
| XL-FEAT-051 | Conditional formatting | Begins with, ends with, blanks, no blanks, errors, dates occurring | Highlight Cells Rules | Absent from `CfRule`. Imported `timePeriod` is kept as an equivalent formula (Degraded). Import names `beginsWith`/`containsBlanks` as omitted (`crates/casual-calc-import/src/lib.rs:190-194`). | P2 | M | |
| XL-FEAT-052 | Conditional formatting | Format is fill-only: no authoring of font style, border or number format | Full dxf | `session_add_cf(..., kind, a, b, text, fill)` takes fill only. A dxf with only border/numFmt is dropped on import (doc 92 R-11). | P2 | M | |
| XL-FEAT-053 | Names | Edit or rename a name, change its refers-to, set sheet scope, add a comment | Name Manager | Go-to and delete only (`webapp/editor.dialogs.js:2405-2422`). `session_define_name` hardcodes `sheet: None` (`crates/casual-calc-wasm/src/objects.rs:1403`). | P2 | S | |
| XL-FEAT-054 | What-if | Data Table (one/two-variable) | `{=TABLE()}` | Absent. A `TABLE()` from a file → unparseable → constants (006). | P2 | M | Finance sensitivity tables. |
| XL-FEAT-055 | What-if | Solver | Simplex LP, GRG Nonlinear, Evolutionary | Absent | P2 | L | |
| XL-FEAT-056 | Calc | Iterative calculation cannot be enabled | File ▸ Options ▸ Formulas | Model has `iteration()` (`crates/casual-calc-model/src/workbook.rs:252`). No wasm binding (grep `iterat` in `crates/casual-calc-wasm/src/` → none). | P2 | S | Engine-capable. |
| XL-FEAT-057 | Functions | REGEXTEST, REGEXEXTRACT, REGEXREPLACE, GROUPBY, PIVOTBY, PERCENTOF, TRIMRANGE | 2024-25 additions | Absent (probe p2: REGEXTEST, GROUPBY, PERCENTOF → `#NAME?`) | P2 | M | |
| XL-FEAT-058 | Functions | FORECAST.ETS family and Forecast Sheet | Present | Absent: FORECAST.ETS, .CONFINT, .SEASONALITY, .STAT. No Forecast Sheet. | P2 | M | |
| XL-FEAT-059 | Data tools | Remove Duplicates: choose which columns define a duplicate | Per-column checklist | `session_remove_duplicates(sheet, first_row, c0, r1, c1)` compares the whole row (`crates/casual-calc-wasm/src/clipboard.rs:1295`). | P2 | S | |
| XL-FEAT-060 | Data tools | Flash Fill (Ctrl+E) | Pattern-inferred fill | Absent (grep `flash` → none) | P2 | L | |
| XL-FEAT-061 | Outline | Data ▸ Subtotal and Auto Outline | Present | Absent. Manual group/levels present (`session_group`, `session_show_outline_level`). | P2 | S | Engine pieces exist (doc 92 C-12). |
| XL-FEAT-062 | Go To | Go To Special (blanks, constants, formulas, errors, differences, precedents, visible cells, CF, DV) | F5 ▸ Special | Only "comments/notes" (`selectCommentedCells`, `webapp/editor.core.js:7276`, Ctrl+Shift+O). The F5 menu has Go To only. | P2 | M | Blanks and Visible Cells are the high-use ones. |
| XL-FEAT-063 | Formula auditing | Evaluate Formula, Error Checking (green triangles), Watch Window | Formulas tab | Trace precedents/dependents and Show Formulas are present (`webapp/ribbon.model.js` Formulas tab, `session_trace`). Evaluate Formula, Error Checking and Watch Window are absent (grep). The `quotePrefix` flag is on the wire and unused (`docs/53` FC-15 🔴). | P2 | M | |
| XL-FEAT-064 | Tables | Slicers (tables and pivots) and timelines | Insert ▸ Slicer / Timeline | Absent (grep). Slicer refs live in `extLst` and are dropped (007). | P2 | L | |
| XL-FEAT-065 | Charts | Sparklines (line, column, win/loss) | Insert ▸ Sparklines | Absent, and they are lost on edited save (007) | P2 | M | |
| XL-FEAT-066 | Charts | Trendlines, error bars, drop/high-low lines, up/down bars | Add Chart Element | Import records `chart/trendline` as a gap (`crates/casual-calc-import/src/chart.rs:223, 401`) | P2 | M | |
| XL-FEAT-067 | Charts | Pivot charts | Insert ▸ PivotChart | Absent | P2 | M | |
| XL-FEAT-068 | Charts | Vertical axis title is not drawn | Rotated axis title | Doc 92 R-13: `crates/casual-calc-layout/src/chart.rs` "Reserved but not drawn" [not re-opened] | P2 | M | Shares the root with 037. |
| XL-FEAT-069 | Number formats | Locale currency and date pickers; Special (zip, phone, SSN) | ~100 currencies, ~17 date types | 14 presets, then a free-text code (`UX-SHELL-13` Done made the code field friendlier) | P2 | M | |
| XL-FEAT-070 | Get & Transform | Power Query: from CSV/folder/web/DB/SharePoint, transform steps, refresh | Data ▸ Get Data | Absent. No network layer by design (`AGENTS.md`). Data ▸ Refresh All refreshes pivots only (`webapp/ribbon.model.js` Data tab). | P2 | XL | |
| XL-FEAT-071 | External links | Formulas referencing other workbooks; Edit Links | `[Book.xlsx]Sheet1!A1`, Data ▸ Edit Links | `=[1]Sheet1!A1` does not parse (probe p8). On edited save the formula becomes its cached constant (probe p9). `externalLink` parts are retained (`crates/casual-calc-import/src/lib.rs:318`). | P2 | M | The loss half is in 006. |
| XL-FEAT-072 | Spelling | Spell check (F7), AutoCorrect | Review ▸ Spelling | Absent. Only `spellcheck=false` attributes appear in the code (`webapp/editor.dialogs.js:257` etc). | P2 | M | |
| XL-FEAT-073 | Accessibility | Accessibility Checker, alt text on charts and images | Review ▸ Check Accessibility | Absent (doc 92 X-11). The grid ARIA mirror is a lead. | P2 | M | |
| XL-FEAT-074 | AI | Copilot-style: natural-language formulas, Analyze Data, summarise, `COPILOT()` function | Copilot in Excel, Analyze Data | Absent (grep `copilot` → none) | P2 | XL | Strategic differentiator. The SDK command registry is a natural tool surface. |
| XL-FEAT-075 | File formats | ODS round trip keeps only values and formulas | Full ODF | By design (`crates/casual-calc-ods/src/lib.rs:1-14`). Losses are counted and askable. | P2 | L | |
| XL-FEAT-076 | Versions | Version history follows the document across machines | Cloud version history | Local IndexedDB only (`webapp/editor.versions.js`, doc 92 P-11) | P2 | L | Same identity decision as 009. |
| XL-FEAT-077 | Save | Save to a real file in browser (File System Access) | Save in place | `SAVE-04` Open P2 | P2 | M | |
| XL-FEAT-078 | Windows | Split panes | View ▸ Split | Absent. Freeze is full (`session_set_freeze`, ribbon View ▸ Freeze Panes). | P2 | M | |
| XL-FEAT-079 | Analysis | Analysis ToolPak (descriptive stats, regression, histogram, ANOVA, t-test output) | Data ▸ Data Analysis | Absent. "Column Statistics" covers a sliver (`webapp/ribbon.model.js` Data ▸ Analysis). | P2 | M | |
| XL-FEAT-080 | Printing | Print titles: repeat columns at left | Columns to repeat | Rows only (`session_set_print_title_rows`, `webapp/editor.dialogs.js:1453`) | P2 | S | |
| XL-FEAT-081 | Entry / formatting | Rich-text runs within a cell (partial bold or colour, super/subscript) | Select characters in edit mode and format | Model carries `TextRun` (`crates/casual-calc-model/src/strings.rs:24`) and the renderer measures runs. Whether in-cell partial formatting is **authorable** is [unverified]. No binding takes runs. | P2 | M | |
| XL-FEAT-082 | Form controls | In-cell checkbox (Insert ▸ Checkbox, 2024) | Boolean cell rendered as checkbox | Absent | P2 | M | Increasingly common in trackers. |
| XL-FEAT-083 | Mobile | Phone/tablet apps | First-party apps | Responsive page only (doc 92 X-12) | P2 | XL | |
| XL-FEAT-084 | Integration | Server-side generation/conversion; REST cell API | Graph workbook API, Office Scripts | All crates `publish = false`. Demo host is unauthenticated (doc 92 P-7, P-13). | P2 | L | |
| XL-FEAT-085 | Tables | Totals row: no default aggregate on the last numeric column | Defaults to SUM on the last column | **Probe p5**: totals row shows "Total" with `session_totals_functions` → `["",""]` | P3 | S | |
| XL-FEAT-086 | AutoFill | `Q1` continues to Q5 instead of wrapping. No custom lists. | Q1…Q4, Q1. Custom lists in Options. | **Probe p5**: `Q1`→Q2, Q3, Q4, **Q5** | P3 | S | Days and months wrap correctly. Numbers, dates and "Item n" also verified. |
| XL-FEAT-087 | What-if | Scenario Manager | Present | Absent | P3 | M | |
| XL-FEAT-088 | Data tools | Consolidate | Data ▸ Consolidate | Absent (grep) | P3 | M | |
| XL-FEAT-089 | Data types | Linked data types (Stocks, Geography, Power BI); STOCKHISTORY | Data ▸ Data Types | Absent | P3 | XL | |
| XL-FEAT-090 | Data model | Power Pivot / Data Model / DAX / CUBE* functions | Present | Absent (CUBEVALUE etc. not in the catalog) | P3 | XL | Enterprise BI users. |
| XL-FEAT-091 | Functions | WEBSERVICE, FILTERXML, ENCODEURL, RTD, CALL, REGISTER.ID, EUROCONVERT, PHONETIC, DBCS, TRANSLATE, DETECTLANGUAGE, IMAGE, PY | Present (some Windows-only) | Absent | P3 | M | Network functions conflict with the no-network design. ENCODEURL is trivial. |
| XL-FEAT-092 | Python | Python in Excel (`=PY()`) | Present | Absent | P3 | XL | |
| XL-FEAT-093 | Formula language | R1C1 reference style (display option and file formulas) | Options toggle | Absent (probe p8: `=R[1]C` fails to parse). OOXML stores A1, so this is display-only. | P3 | M | |
| XL-FEAT-094 | Windows | New Window, Arrange All, View Side by Side, Synchronous Scrolling, Custom Views | View tab | Absent. Personal filter views partly cover Sheet Views (`crates/casual-calc-wasm/src/view.rs:10-37`). | P3 | L | |
| XL-FEAT-095 | View | Zoom range 10–400% and Zoom to Selection | 10–400% | `ZOOM_MIN = 0.25, ZOOM_MAX = 2` (`webapp/editor.core.js:1589`). The ribbon offers 50/75/100/150/200. | P3 | S | |
| XL-FEAT-096 | Formatting | Shrink-to-fit control | Format Cells ▸ Alignment | Renders, but there is no setter or checkbox (doc 92 R-14) | P3 | S | |
| XL-FEAT-097 | Formatting | Pattern fills, gradient-fill authoring, diagonal borders authoring | Format Cells ▸ Fill/Border | Model has `GradientFill`. No pattern or gradient UI (grep). Diagonal mentioned in the renderer only. [authoring unverified] | P3 | M | |
| XL-FEAT-098 | Styles / themes | New Cell Style, Modify Style, workbook Themes (colours, fonts, effects) | Page Layout ▸ Themes | 10 built-in styles applied by name (`session_apply_cell_style`). Theme colours read, never written (doc 92 R-12). | P3 | M | |
| XL-FEAT-099 | Sheets | Group sheets (edit several selected sheets at once) | Ctrl+click tabs | Absent [unverified, no multi-sheet selection found] | P3 | M | |
| XL-FEAT-100 | Data validation | Circle Invalid Data | Data ▸ Data Validation ▸ Circle | Absent (grep `circle` → none) | P3 | S | |
| XL-FEAT-101 | Printing | Header/footer: field codes UI (page, date, file path), images, different first/odd-even | Header & Footer tools | Free-text odd header/footer fields only (`webapp/editor.dialogs.js:1458-1463`). The field-code language exists (`IO-11`). | P3 | S | |
| XL-FEAT-102 | Paste | Paste Special: skip blanks, paste link, paste as picture | Present | All/values/formulas/formats/widths plus add/subtract/multiply/divide and transpose are present (`webapp/editor.dialogs.js` `pasteSpecialDialog`). Skip blanks and paste link are absent. | P3 | S | |
| XL-FEAT-103 | Find | Find by format; Replace across all sheets; search in comments | Present | `session_find_opts` (match case, whole cell, values, all sheets, wildcards). `session_replace_all(sheet, …)` is per sheet. No format search. | P3 | S | |
| XL-FEAT-104 | Linked picture | Camera tool / Paste as Linked Picture | Present | Absent | P3 | M | |
| XL-FEAT-105 | Form controls | Form controls (button, combo, spin, option) and ActiveX | Developer tab | Absent. Retained in `.xlsm` [unverified] | P3 | L | |
| XL-FEAT-106 | Compare | Inquire / Spreadsheet Compare | Add-in | Absent | P3 | L | |
| XL-FEAT-107 | Calc | Multi-threaded recalculation | Uses all cores | Single-threaded wasm, no Worker | P3 | XL | Covered by 042 for the main-thread half. |
| XL-FEAT-108 | Data | Data Form; Stock/Geography "Insert Data" | Present | Absent | P3 | S | Rare. |
| (parity) | Hyperlinks | (parity) | Ctrl+K, HYPERLINK() | Present: `session_set_hyperlink` and the `HYPERLINK` function (probe p2 returns its label) | — | — | Listed for completeness. No gap. |

### Areas verified as present (no row, for the reader's confidence)

| Area | Evidence |
|---|---|
| Dynamic arrays core | **Probe p3**: SEQUENCE, FILTER, UNIQUE, SORT (explicit args), SORTBY, MAP, BYROW, LET and LAMBDA-call all spill correctly. `#SPILL!` when blocked. |
| Lookup & conditional aggregates | **Probe p3**: XLOOKUP, VLOOKUP, COUNTIF, SUMIFS, INDEX/MATCH with an array criterion, INDIRECT, OFFSET, TEXT with `#,##0.00`, `0%`, `h:mm AM/PM`, NETWORKDAYS. |
| Data validation | `session_set_validation` (kinds via `DvKind::from_ooxml`), messages and styles (`crates/casual-calc-wasm/src/data.rs:361-435`). |
| Tables | Create, style, totals, rename, header toggle, autoexpand (probe p5, `session_*table*` exports). |
| Freeze panes, hide/unhide, outline, merge, borders, rotation, indent, wrap | Exports `session_set_freeze`, `session_group`, `session_merge_cells`, `session_set_rotation`, … |
| Paste special (values/formats/ops/transpose) | `webapp/editor.dialogs.js` `pasteSpecialDialog` |
| Fill: series, growth, formatting-only, without formatting | `webapp/ribbon.model.js` Editing ▸ Fill menu. Probe p5. |
| Page setup: print area, title rows, margins, orientation, fit-to, header/footer, manual breaks, PDF | `session_set_page_setup` keys hf/margins/options/page/setupPr (`crates/casual-calc-wasm/src/view.rs:202`). `session_export_pdf`. |
| Threaded comments round trip | `crates/casual-calc-import/src/read.rs:424`, `crates/casual-calc-export/src/lib.rs:393` |
| Command search (Alt+Q) | `UX-SHELL-15` Done. `webapp/ribbon.js:376-389` |
| Grid size | `crates/casual-calc-formula/src/reference.rs:43-45` (1,048,576 × 16,384) |

---

## Beyond Excel: the external review (2026-10-15)

A review that diffed OpenCalc against a comparable open-source spreadsheet. Its function
list was confirmed by searching our crates; its other points were checked against the
repository. Everything real is a row; the owner put it in this roadmap.

| Row | Item | Decision |
|---|---|---|
| XL-FEAT-012, -013 | The missing functions it listed (XOR, AGGREGATE, the T/F/CHISQ family, BASE/DECIMAL/ARABIC, the IM* set, VSTACK/HSTACK/TAKE/DROP and the 2022 family) | In progress; each tested against LibreOffice and Microsoft's examples, and added to the CI oracle corpus |
| XL-FEAT-119 | GROUPBY, PIVOTBY | Build, after LAMBDA-as-argument is confirmed |
| XL-FEAT-120 | WEBSERVICE, FILTERXML, CUBE* | **Not built**: request forgery from a recalculation on a shared server; absent from Excel for the web and Mac; CUBE needs a data model. Cached values kept and named |
| XL-FEAT-121 | PHONETIC, DBCS | With CJK support |
| XL-FEAT-122 | Agent access — MCP server, CLI, live participant | **Accepted**: [docs/97](97-AGENT-ACCESS.md), rows AGENT-01..04 |
| XL-FEAT-123 | A native (egui) desktop UI instead of the web shell in Tauri | **Not adopted**: a second UI to build and keep identical; the one web shell serves browser, desktop, embeds and collaboration. The desktop's real gaps are signing (XL-FEAT-044) and native menus, not the toolkit |
| XL-FEAT-035 | `.xlsb` (and `.xls`) | Already a P1 row; the review's ODS-import point does not apply — `.ods` opens today (`casual-calc-ods`) |
| XL-UX-157 | Cross-check our ribbon coverage against the reviewed project's parity checklists | Needs access to that repository |

## UX gaps (`XL-UX`)

| ID | Surface | Inconsistency / gap | Excel does | OpenCalc does (evidence) | Pri | Eff |
|---|---|---|---|---|---|---|
| XL-UX-001 | Grid / Show Formulas | Ctrl+` shows `#REF!` for every relative formula | Shows each formula as typed (`=B2*C2`, `=SUM(D2:D4)`), left-aligned, columns widened | `F!*#REF!`, `=SUM(#REF!:#REF!)` (ux-audit/showformulas.png, k2-Control_Backquote-crop.png). `crates/casual-calc-wasm/src/axis.rs:587` calls `cell_input_text` (`lib.rs:222`), which formats `wb.formula(handle)` with no origin; export uses `formula_text(expr, origin)` (`casual-calc-export/src/lib.rs:482`) | P0 | S |
| XL-UX-002 | Keyboard / Delete | Ctrl+- on a single cell deletes the whole row with no prompt | Opens the Delete dialog (Shift cells left / Shift cells up / Entire row / Entire column) | B2 selected, Ctrl+- → row 2 gone, total 43.48→29.98 (k2-Control_Minus.png) | P0 | M |
| XL-UX-003 | Formula edit / Point→Edit | F2 does not toggle Point/Enter→Edit; arrows then corrupt the formula | F2 toggles Enter/Point ↔ Edit; in Edit mode arrows move the caret | `=SUBSTITUTE($A$3:$A$5` → F2 → ← gives `=SUBSTITUTE(A3$A$5`; mode stays "Point" (a4.cjs output; a10.cjs: `=B3` stays Point after F2+←) | P0 | M |
| XL-UX-004 | Ribbon popups / Escape | Esc does not close the Orientation or Fill/Font colour popups opened from the ribbon | Esc closes the open gallery/menu and returns focus | After Esc the popup stays (`popmenu`, `popmenu color-menu` still visible) and lingers across later actions (a3.cjs; dd-all.png shows the Orientation menu persisting) | P0 | S |
| XL-UX-005 | Keyboard / Insert | Ctrl+Shift+= inserts an entire row with a single cell selected | Opens the Insert dialog (Shift cells right/down, Entire row/column) | Row 2 inserted, data shifted down (k2-Control_Shift_Equal.png) | P1 | M |
| XL-UX-006 | Ribbon labels | Large buttons and group captions are hidden | Every large button has a text label; every group has a caption | Icons only; labels exist only behind File > Options > Ribbon labels (tabs-all.png; `webapp/ribbon.css:118-155`) **[owner decision, docs/95 rule 1]** | P1 | S |
| XL-UX-007 | Ribbon / dialog launchers | Group dialog launchers are `display:none` by default | Font, Alignment, Number and Clipboard groups have a corner launcher (Format Cells / Clipboard pane) | `webapp/ribbon.css:123` hides `.rb-launch` unless captions are on, so Format Cells has no mouse path from Home | P1 | S |
| XL-UX-008 | Ribbon / ScreenTips | No ScreenTips: only the native `title` attribute | Styled ScreenTip with bold name, shortcut, one-line description and "Tell me more"; appears after ~0.5 s | `b.title = spec.label` (`webapp/ribbon.js:166`); no tooltip rendered (tip-bold.png) | P1 | M |
| XL-UX-009 | KeyTips | Only one KeyTip level (tabs) | Alt → tab letters → per-control KeyTips (e.g. Alt H B for Borders), QAT digits | After Alt, H nothing is badged on controls (keytips-H.png; `webapp/ribbon.js:712-750` handles only F and tab keys) | P1 | M |
| XL-UX-010 | Ribbon responsive | Folded groups are unlabelled icons | A collapsed group becomes one large button with the group name and a caret | 1024 px: Styles/Cells/Editing fold to 3 bare icons; 768 px: 6 bare icons (resp.png; a8.cjs `folded` list) | P1 | S |
| XL-UX-011 | Selection / Shift+click | Shift+click moves the active cell to the clicked cell | The active cell stays at the anchor (B2) | Click B2, Shift+click C3 → name box `C3` (a10.cjs) | P1 | S |
| XL-UX-012 | Selection / Enter in range | Enter leaves a multi-cell selection | Enter/Tab cycle inside the selected range | B2:C3 selected, Enter ×4 → C4, C5, C6, C7 (a10.cjs) | P1 | M |
| XL-UX-013 | Selection / Ctrl+A | Ctrl+A moves the active cell to the region's last cell | Selects the current region; the active cell does not move | From B2 → name box D5 (a5.cjs: Control+a) | P1 | S |
| XL-UX-014 | Name box | Shows "3R x 2C" after the selection is finished | Shows rows×cols only while dragging, then the active cell | `3R x 2C` persists after Shift+arrows (g-all.png bottom row, a4.cjs) | P1 | S |
| XL-UX-015 | Context menu / keyboard | ContextMenu key opens at the mouse and moves the selection there; Shift+F10 does nothing | Both open the cell menu at the active cell without moving the selection | B2 → menu opens at K16 and selection becomes K16 (k2-ContextMenu.png); Shift+F10 no menu (k2-Shift_F10.png) | P1 | S |
| XL-UX-016 | Error indicators | Indicator corners and colours are inverted | Green triangle **top-left** = error-checking (numbers stored as text, #DIV/0!, inconsistent formula); red **top-right** = note/comment | Number-as-text green triangle drawn top-right (`webapp/editor.core.js:3361-3384`, whose comment states the wrong convention); red marker top-left on #DIV/0!/#NAME? (g-errors in g-all.png) | P1 | S |
| XL-UX-017 | Cell AutoComplete | No AutoComplete from the column's existing text | Typing "Wid" under "Widget" completes to "Widget" with the tail selected | Cell shows only "Wid" (g-all.png top-left) | P1 | M |
| XL-UX-018 | Fonts / formula bar | Formula bar, name box and in-cell editor are monospace | Formula bar and name box use the UI sans font; the in-cell editor uses the cell's font | `ui-monospace, SFMono-Regular, Menlo…` for `#cell-ref` and the formula textarea (a14.cjs); in-cell "=SUBSTITUTE(" is monospace (g-all.png) | P1 | S |
| XL-UX-019 | Go To | Ctrl+G / F5 / Find & Select › Go To open no dialog | Go To dialog (recent references, named ranges, Reference box, Special… button) | `edit.go-to` only focuses the name box (a13.cjs: focus `cell-ref`; k2-Control_g.png, k2-F5.png) | P1 | M |
| XL-UX-020 | Go To Special | Missing | Go To Special (Blanks, Constants, Formulas, Visible cells only, Current region…) | Not in Find & Select menu or registry (dd-Find_Select.png; command list a1.json) | P1 | M |
| XL-UX-021 | Name Manager | Ctrl+F3 opens a small popover, not a dialog | Name Manager dialog: New/Edit/Delete, Filter, columns Name/Value/Refers To/Scope/Comment | Popover "NO NAMED RANGES YET" anchored near the name box (k2-Control_F3.png; `webapp/editor.dialogs.js:2396`) | P1 | M |
| XL-UX-022 | Define Name | Formulas › Define Name opens the same popover | New Name dialog (Name, Scope, Comment, Refers to) | Popover "No named ranges yet" (dlg-formulas_define_name.png) | P1 | M |
| XL-UX-023 | Protect Sheet | Protect Sheet toggles instantly with no dialog | Protect Sheet dialog: password, "Allow all users of this worksheet to" checklist | Status "sheet protected — locked cells refuse edits"; no dialog (dlg-format_protection_protect_this_sheet.png) | P1 | M |
| XL-UX-024 | Find & Replace | Ctrl+F/Ctrl+H open a floating bar | Find and Replace dialog with Find/Replace tabs, Options >> (Within, Search, Look in, Match case/entire cell), Find All results list | 852×44 bar at top-right of the grid (findbar.png, k2-Control_f.png) | P1 | L |
| XL-UX-025 | Find bar | Options are five unlabeled icon checkboxes | Labelled checkboxes and dropdowns | Checkbox + glyphs "Ab", ▦, #, ⧉, * with only `title` text (findbar.png; `webapp/editor.html:564-568`) | P1 | S |
| XL-UX-026 | Insert Function | Shift+F3 does nothing | Opens the Insert Function dialog | No dialog (k2-Shift_F3.png) | P1 | S |
| XL-UX-027 | Insert Function dialog | Flat A–Z list with no category, description or Function Arguments step | Search + Go, "Or select a category", list, syntax and description, Help link, OK → Function Arguments dialog | Search box + list + Insert/Cancel; no description (dlg-m1.png, ifn-search.png) | P1 | L |
| XL-UX-028 | Backstage | File page is a list of text links | Home/New template tiles, Open (Recent + locations), Info (Protect/Inspect/Version History tiles + properties pane), Save As (location + file-type dropdown + Save), Print (preview + settings), Export, Options dialog | Rail plus plain link lists (file-all.png) | P1 | L |
| XL-UX-029 | Backstage / Print | No print preview or settings in File › Print | Live preview with printer, pages, orientation, paper, margins and scaling dropdowns | "Print…", "Export as PDF…", "Page setup…" links (file-Print.png) | P1 | L |
| XL-UX-030 | Status bar | No right-click customise menu | Right-click lists Cell Mode, Caps/Num Lock, Average, Count, Numerical Count, Min, Max, Sum, View Shortcuts, Zoom Slider… as toggles | Right-click shows nothing (status-rclick in misc.png) | P1 | M |
| XL-UX-031 | Status bar | No view buttons | Normal / Page Layout / Page Break Preview buttons left of the zoom slider | Zoom −/slider/+ and 100% only (01-default-1920.png) | P1 | M |
| XL-UX-032 | Status bar | Transient messages linger and read as debug text | Status bar shows mode plus brief, Excel-worded prompts only | "ok" after commit (`webapp/editor.selection.js:598`), "resized", "copied here — the system clipboard was refused — other applications will not see it" persisting across later selections (a4.cjs, misc.png) | P1 | S |
| XL-UX-033 | Status bar layout | Status bar and sheet tabs share one row | Sheet-tab row (nav arrows, tabs, ⊕, horizontal scrollbar) above a separate status bar (Ready … views, zoom) | "Ready" sits right after the tabs in one bar (01-default-1920.png) | P1 | M |
| XL-UX-034 | Cell context menu | Wording, order and items differ from Excel | Cut, Copy, Paste Options (icons), Paste Special…, Smart Lookup, Insert…, Delete…, Clear Contents, Quick Analysis, Filter ›, Sort ›, New Comment, New Note, Format Cells…, Pick From Drop-down List…, Define Name…, Link | Sentence case; duplicate "Insert cells ›" + "Insert ›", "Delete cells ›" + "Delete ›"; "Hide ›", "Clear ›", "Insert comment", "Create table…" (k2-ContextMenu.png; a6.cjs list) | P1 | M |
| XL-UX-035 | Row Height / Column Width | Units are pixels | Row height in points (15); column width in characters (8.43) | "Height 20 px" (dlg-m1.png); "Width px" (a7.cjs) | P1 | M |
| XL-UX-036 | Auto-widen | Entering a date/number shows ##### instead of widening the column | Excel widens a General-width column to fit a date or number entry | Ctrl+; in B2 → `#####` (k2-Control_Semicolon.png); Ctrl+Shift+^ scientific → `#####` | P1 | M |
| XL-UX-037 | Home › Conditional Formatting | Only two items | Highlight Cells Rules ›, Top/Bottom Rules ›, Data Bars ›, Color Scales ›, Icon Sets ›, New Rule…, Clear Rules ›, Manage Rules… | "Conditional formatting…", "Conditional formatting rules…" (dd-all.png) | P1 | L |
| XL-UX-038 | Insert tab | Missing groups | Tables (incl. Recommended PivotTables), Illustrations, Add-ins, Charts (Recommended Charts, gallery), Tours, Sparklines, Filters (Slicer, Timeline), Links, Comments, Text (Text Box, Header & Footer), Symbols | Tables, Charts (4 small), Links, Comments only (tab-insert.png) | P1 | L |
| XL-UX-039 | Notes vs comments | "New Note" / Shift+F2 open a threaded Comments panel | Shift+F2 / New Note opens a sticky-note box on the cell; Comments are a separate feature | Side panel "Comments … Save/Resolve/Delete" (dlg-insert_note.png, k2-Shift_F2.png) | P1 | M |
| XL-UX-040 | Page Layout tab | Missing page-setup controls | Themes; Page Setup: Margins, Orientation, Size, Print Area, Breaks, Background, Print Titles; Scale to Fit; Sheet Options; Arrange | 3 large buttons (Page Setup, Insert Page Break, Print) + 2 icons (tab-pagelayout.png) | P1 | L |
| XL-UX-041 | Formulas tab | No function library categories | Recently Used, Financial, Logical, Text, Date & Time, Lookup & Reference, Math & Trig, More Functions dropdowns | Insert Function + AutoSum only (tab-formulas.png) | P1 | M |
| XL-UX-042 | Review tab | Missing groups | Proofing (Spelling, Thesaurus, Workbook Statistics), Accessibility, Language, Comments (New, Delete, Previous, Next, Show), Notes, Protect (Protect Sheet, Protect Workbook, Allow Edit Ranges) | New Note, Protect Sheet, Lock Cell, Hide Formula (tab-review.png) | P1 | L |
| XL-UX-043 | View tab | Missing groups | Sheet View (Keep/Exit/New/Options), Workbook Views (Normal, Page Break Preview, Page Layout, Custom Views), Show (Ruler, Gridlines, Formula Bar, Headings), Zoom (Zoom, 100%, Zoom to Selection), Window (New Window, Arrange All, Freeze Panes, Split, Hide/Unhide, Side by Side) | Exit Sheet View, Gridlines/Headings/Zero Values, Zoom, 100%, Freeze Panes, Version History (tab-view.png) | P1 | L |
| XL-UX-044 | Format Cells | Five tabs; Protection missing | Number, Alignment, Font, Border, Fill, Protection, in that order | Number, Font, Alignment, Fill, Border (dlg-m1.png; `editor.dialogs.js:177,342`) | P1 | M |
| XL-UX-045 | Error indicators | No error smart tag on a cell with an indicator | Selecting the cell shows a ⚠ button (Convert to Number, Help on this error, Ignore Error…) | Not observed when an error cell was selected **[unverified: not explicitly hovered]** | P2 | M |
| XL-UX-046 | Find feedback | No-match feedback is a bare "0" | "We couldn't find what you were looking for." message box | Bar shows `0` (a13.cjs, findbar-none.png) | P2 | S |
| XL-UX-047 | Insert Function dialog | Enter after a search does not pick the single match | The first match is selected; Enter/OK proceeds | "vlook" → VLOOKUP listed but not highlighted; Enter inserts nothing (a13.cjs fb empty) | P2 | S |
| XL-UX-048 | Backstage / Save | "Save a Copy" pane mixes "Save" with format names | Save As: name field + file-type dropdown (Excel Workbook (*.xlsx), CSV UTF-8, OpenDocument Spreadsheet…) + Save | Links "Save, Excel, OpenDocument, CSV, Tab-separated, …, Same format as opened" (file-Save_a_Copy.png) | P2 | M |
| XL-UX-049 | Backstage / Options | Options is a theme list plus a ribbon-labels list | Options dialog with categories (General, Formulas, Data, Proofing, Save, Language, Accessibility, Advanced, Customize Ribbon, QAT) | "Auto/Light/Dark/Settings…" + "Ribbon labels" with no section headings (file-Options.png) | P2 | L |
| XL-UX-050 | Backstage | No Home, Export, Close or Account entries | Rail: Home, New, Open, Info, Save, Save As, Print, Share, Export, Publish, Close, Account, Options | `FILE_RAIL` = New, Open, Info, Save a Copy, Print, Share, Options (`webapp/ribbon.model.js` FILE_RAIL) | P2 | M |
| XL-UX-051 | Status bar | Shows "Local only · nothing uploaded" | No storage text (AutoSave state lives in the title bar) | Persistent green dot + text (01-default-1920.png) | P2 | S |
| XL-UX-052 | Grid scrollbars | No horizontal scrollbar on desktop | Horizontal scrollbar shares the tab row; vertical at right | Only a vertical bar at 1440 px; a horizontal bar appears only on phone (k2-ContextMenu.png vs w390.png) | P2 | M |
| XL-UX-053 | Sheet tabs | ⊕ sits left of the tabs, plus a "≡ All sheets" button | ⊕ New sheet after the last tab; right-click on the nav arrows lists all sheets | `+` and `≡` at far left (many-sheets2.png) | P2 | S |
| XL-UX-054 | Sheet tabs | Inactive tabs bold; active tab has a coloured dot; overflowed tab text clipped mid-glyph | Inactive tabs regular weight; active tab white with an accent underline and no dot; tabs scroll whole | All tabs bold, "• Sheet15", "ıeet3" clipped (many-sheets2.png) | P2 | S |
| XL-UX-055 | Sheet tab menu | Items and order differ | Insert…, Delete, Rename, Move or Copy…, View Code, Protect Sheet…, Tab Color ›, Hide, Unhide…, Select All Sheets | Rename, Duplicate, inline "TAB COLOR" 8 swatches, Protect sheet, Hide sheet, Delete in red (ctx-all.png) | P2 | M |
| XL-UX-056 | Cell context menu | No mini toolbar | Right-click shows the floating formatting mini toolbar above the menu | Menu only (k2-ContextMenu.png) | P2 | M |
| XL-UX-057 | Cell context menu | No Paste Options icon row | Paste Options row of icons (Paste, Values, Formulas, Transpose, Formatting, Link) | Text "Paste" + "Paste special ›" submenu (k2-ContextMenu.png) | P2 | M |
| XL-UX-058 | Column/row header menu | Items differ | Cut, Copy, Paste Options, Paste Special…, Insert, Delete, Clear Contents, Format Cells…, Column Width…/Row Height…, Hide, Unhide | "Insert 1 column left/right", red "Delete 1 column", "Autofit width", no Format Cells or Paste Special (ctx-all.png) | P2 | S |
| XL-UX-059 | Formula error | A syntax error stays in the cell with a red box and a status-bar sentence | Modal "There's a problem with this formula" with OK and the offending part selected | `=A1+` → cell stays editing, status "Formula error: the formula looks incomplete…" (g-errors in g-all.png) | P2 | M |
| XL-UX-060 | Formula bar | No Cancel ✗ / Enter ✓ buttons | ✗ ✓ fx beside the name box (greyed in Ready) | fx only (01-default-1920.png) | P2 | S |
| XL-UX-061 | Formula bar | Placeholder text in empty cells | Empty formula bar | "value or =formula" (`webapp/editor.html:538`; docname/fbar in misc.png) | P2 | S |
| XL-UX-062 | Name box | Going to `C3:D4` makes D4 active | The active cell is the first cell typed (C3) | Name box shows D4 (a6.cjs) | P2 | S |
| XL-UX-063 | Name box | Defining a name by typing it in the name box does not show the name | Name box shows "MyRange" while that range is selected | Shows "D4" after defining "MyRange" (a6.cjs) | P2 | S |
| XL-UX-064 | Point mode visual | Referenced range drawn as a solid box | Moving dashed marquee in the reference colour while pointing | Solid blue rectangle (g-point2 in g-all.png) | P2 | S |
| XL-UX-065 | Formula AutoComplete | List shows monospace names with full signatures inline; no description tip | Function icon + name; description ScreenTip beside the highlighted item | Monospace "SUBSTITUTE (text, old, new, [instance])" rows (g-fac in g-all.png) | P2 | S |
| XL-UX-066 | Edit keys | Backspace in Ready clears the cell but stays in Ready | Backspace clears and enters Edit mode with the caret in the empty cell | A2 → Backspace → Ready, empty (a10.cjs) | P2 | S |
| XL-UX-067 | Edit keys (macOS) | Home/Ctrl+Home do nothing in the in-cell editor | Home moves the caret to the start | F2, Home, type X → `=B2*C2X` (a11.cjs) **[mac-only; Windows may be native]** | P2 | S |
| XL-UX-068 | Fill handle | No Auto Fill Options smart tag after a drag | Smart tag (Copy Cells, Fill Series, Fill Formatting Only, Fill Without Formatting, Flash Fill) | No tag after the drag (fill-after in misc.png) | P2 | M |
| XL-UX-069 | Paste | No Paste Options smart tag after paste | Ctrl smart tag at the paste range | Not observed **[unverified: system clipboard refused in headless]** | P2 | M |
| XL-UX-070 | Copy | Clipboard message is wordy and sticky | Marching ants plus "Select destination and press ENTER or choose Paste" | Long refusal sentence persists in the status bar (a4.cjs) | P2 | S |
| XL-UX-071 | Home › Paste menu | Text list, not icon gallery | Paste Options gallery: Paste, Formulas, Formulas & Number Formatting, Keep Source Formatting, No Borders, Keep Column Widths, Transpose, Values…, Other Paste Options, Paste Special… | "Paste, Paste Values, Paste Formulas, Paste Formatting, Transpose, Paste Special…" (dd-all.png) | P2 | M |
| XL-UX-072 | Home › Borders | Icon grid instead of Excel's named list | List: Bottom Border, Top Border, Left, Right, No Border, All Borders, Outside Borders, Thick Outside, … Draw Borders, Line Color, Line Style, More Borders… | 4×3 icon grid + "Thin" select + colour dots (dd-all.png) | P2 | M |
| XL-UX-073 | Conditional Formatting UI | Side panel instead of dialogs | New Formatting Rule dialog and Rules Manager dialog | 321 px side panel (dlg-m2.png) | P2 | L |
| XL-UX-074 | Home › Cell Styles | Opens a modal dialog | In-ribbon gallery dropdown with live preview | Modal "Cell styles" with Close (dd-all.png) | P2 | M |
| XL-UX-075 | Home › Insert menu | Items and wording differ | Insert Cells…, Insert Sheet Rows, Insert Sheet Columns, Insert Sheet | "Rows above, Rows below, Columns left, Columns right, Insert Cells, Shift Down, …, Insert Sheet" (dd-all.png) | P2 | S |
| XL-UX-076 | Home › Format menu | Flat list without Excel's sections | Sections Cell Size / Visibility (Hide & Unhide ›) / Organize Sheets / Protection; includes Default Width…, Tab Color, Move or Copy Sheet…, Lock Cell | Flat list "Row Height…, AutoFit Row Height, …, Rename Sheet, Protect this sheet, Format Cells…" (dd-all.png) | P2 | S |
| XL-UX-077 | Home › AutoSum menu | Only AutoSum + Insert Function | Sum, Average, Count Numbers, Max, Min, More Functions… | 2 items (dd-all.png) | P2 | S |
| XL-UX-078 | Home › Fill menu | Sentence case; missing items | Down, Right, Up, Left, Across Worksheets…, Series…, Justify, Flash Fill | "Fill down, Fill right, Fill series, Growth series, Copy cells, Formatting only, Without formatting" (dd-all.png) | P2 | S |
| XL-UX-079 | Home › Clear menu | Missing items; wording | Clear All, Clear Formats, Clear Contents, Clear Comments and Notes, Clear Hyperlinks | "All, Formatting, Values" (dd-all.png) | P2 | S |
| XL-UX-080 | Home › Sort & Filter menu | Wording; missing Reapply | Sort A to Z, Sort Z to A, Custom Sort…, Filter, Clear, Reapply | "A→Z, Z→A, Custom sort…, Filter, Clear all filters" (dd-all.png) | P2 | S |
| XL-UX-081 | Home › Find & Select menu | Missing items | Find…, Replace…, Go To…, Go To Special…, Formulas, Comments, Notes, Conditional Formatting, Constants, Data Validation, Select Objects, Selection Pane… | "Find & replace…, Replace…, Go To…, Select all" (dd-all.png) | P2 | M |
| XL-UX-082 | Home › Number Format list | No previews | Each format shows an icon and the selected cell's value previewed | Text names only (dd-numfmt.png) | P2 | S |
| XL-UX-083 | Home › Number group | "Currency Format" button instead of Accounting split button | Accounting Number Format with a currency dropdown ($ English, £, €, More Accounting Formats…) | Plain button "Currency Format" (`webapp/ribbon.model.js:58`) | P2 | S |
| XL-UX-084 | Insert › Comments | "Note" where Excel has Comment | Insert › Comment (threaded); Notes live under Review › Notes | `L("insert.note", …, "Note")` (`ribbon.model.js:89`) | P2 | S |
| XL-UX-085 | Page Layout › Sheet Options | Gridlines/Headings are icon toggles | View ☑ / Print ☑ checkboxes under Gridlines and Headings | Two unlabeled small icons (tab-pagelayout.png) | P2 | S |
| XL-UX-086 | Page Setup | Side panel instead of dialog | Page Setup dialog with Page / Margins / Header/Footer / Sheet tabs | 321 px side panel (dlg-file_page_setup.png) | P2 | L |
| XL-UX-087 | Formulas tab | Defined Names, Auditing and Calculation incomplete | Use in Formula, Create from Selection; Error Checking, Evaluate Formula, Watch Window, Show Formulas; Calculate Sheet | Name Manager, Define Name; Trace ×2, Remove Arrows, Show Formulas; Calc Options, Calculate Now (ribbon.model.js:102-122) | P2 | L |
| XL-UX-088 | Data tab | Missing groups and commands | Get & Transform Data, Queries & Connections, Data Types, Sort & Filter (+Reapply, Advanced), Data Tools (Flash Fill, Consolidate, Relationships), Forecast (What-If, Forecast Sheet), Outline (+Subtotal) | Refresh All, Sort/Filter, 4 tools, Outline, "Analysis: Column Statistics" (tab-data.png) | P2 | L |
| XL-UX-089 | Data › Data Validation | Side panel instead of dialog | Data Validation dialog: Settings / Input Message / Error Alert tabs | Side panel "Data validation" (dlg-m2.png) | P2 | M |
| XL-UX-090 | View tab | Extraneous and misplaced items | Zero Values lives in Options › Advanced; Version History in File › Info; "Exit" only when a Sheet View is active | Zero Values on View › Show; Collaboration › Version History; Exit Sheet View always shown (`ribbon.model.js:155-170`) | P2 | S |
| XL-UX-091 | Ribbon icons | One icon reused for unrelated commands | Each command has distinct artwork | `table_delete_row` for Delete, Ungroup and Remove Duplicates; `table_cells_merge` for Merge and Group; `eye` for Show Formulas, Hide Formula, Sheet View, Show; `table_settings` for Format and Name Manager; `checkmark` for Data Validation (`ribbon.model.js`) | P2 | M |
| XL-UX-092 | Ribbon collapse | Ctrl+F1 and double-clicking a tab do nothing | Both toggle "Show tabs only" | Ribbon unchanged (ctrlF1/dbltab in misc2.png) | P2 | S |
| XL-UX-093 | Ribbon collapse | The ˄ chevron at bottom-right switches to Simplified | ˄ collapses the ribbon; Ribbon Display Options (Always show / Tabs only / Full-screen, Classic/Simplified) | `aria-label="Show a simplified ribbon"` on the chevron (a14.cjs; `ribbon.js:689`) | P2 | S |
| XL-UX-094 | Search (Alt+Q) | Alt+Q raises KeyTip badges over the search | Alt+Q focuses Search; no KeyTips | Tab KeyTips M/A/R/W/Y drawn while typing (search in misc2.png) | P2 | S |
| XL-UX-095 | Title bar / QAT | No AutoSave toggle; Save icon appears late | QAT: AutoSave toggle, Save, Undo▾, Redo▾, customise ▾ always present | At 1920 px after 2.5 s: logo, Undo, Redo only (01-default-1920.png); later shots show Save (k2-ContextMenu.png) | P2 | S |
| XL-UX-096 | Title bar / QAT | No QAT customise dropdown; Undo/Redo lack history dropdowns | ▾ "Customize Quick Access Toolbar"; Undo/Redo split buttons with history lists | Plain icon buttons (01-default-1920.png) | P2 | M |
| XL-UX-097 | Title bar | No Comments button; Settings gear not Excel | Right side: Comments, Share (gear is not on the title bar) | Share + gear (01-default-1920.png; docs/95 lists Comments as partial) | P2 | S |
| XL-UX-098 | Title bar | Default name and window title | New workbook "Book1"; window title "Book1 - Excel" | "Untitled workbook"; `document.title` "OpenCalc — editor demo" (a9.cjs) | P2 | S |
| XL-UX-099 | Format Cells › Number | "Special" category missing; list clipped | 12 categories incl. Special, all visible | 11 categories; "Text" cut off at the bottom with no scroll cue (dlg-m1.png, dark-dlg.png) | P2 | S |
| XL-UX-100 | Format Cells | Primary button reads "Apply" | OK / Cancel | Apply / Cancel (dlg-m1.png) | P2 | S |
| XL-UX-101 | Paste Special | Options subset; wording | All, Formulas, Values, Formats, Comments and Notes, Validation, All using Source theme, All except borders, Column widths, Formulas and number formats, Values and number formats, All merging conditional formats; Operation; Skip blanks, Transpose; Paste Link | Everything, Values only, Formulas, Formats only, Column widths only; ops; Transpose; no Skip blanks or Paste Link (dlg-m1.png) | P2 | M |
| XL-UX-102 | Sort dialog | Fixed 3 levels; text-only ordering | Add/Delete/Copy Level, Options…, columns Column / Sort On / Order ("Smallest to Largest" for numbers), "My data has headers" | Sort by / Then by ×2, "A → Z" for numeric Qty, title "Sort range" (dlg-m1.png) | P2 | M |
| XL-UX-103 | Hyperlink dialog | Different structure and labels | Link to: Existing File or Web Page / Place in This Document / Create New Document / E-mail Address; Text to display at top; ScreenTip… button | Stacked fields: Web address, Place in this workbook ("Sheet2!A1" placeholder with no Sheet2), Text to display, "Tooltip" (dlg-m1.png) | P2 | M |
| XL-UX-104 | Dialog titles | Title Case and sentence case mixed | Title Case: Paste Special, Insert Function, Sort, Create Table, Convert Text to Columns Wizard, Remove Duplicates | "Format Cells", "Row Height", "Insert Hyperlink" vs "Paste special", "Insert function", "Sort range", "Create table", "Text to columns", "Remove duplicates", "Conditional formatting rules", "Document properties", "Cell styles" (a7.cjs) | P2 | S |
| XL-UX-105 | Menu wording | Registry menu labels in sentence case | Title Case with ellipsis for dialogs ("Page Setup…", "Find…", "Version History") | "Page setup…", "Find & replace…", "Version history…", "Select all", "Export as PDF…" (a1.json `menuModel`) | P2 | S |
| XL-UX-106 | Side-panel style | Several Excel dialogs are side panels with different chrome | Modal dialogs with OK/Cancel | CF, DV, Page Setup, Pivot, Chart, Comments, Version History, Column stats share one 321 px panel; Apply/Remove/Close buttons vary (a7.cjs) | P2 | L |
| XL-UX-107 | Remove Duplicates | No column checklist | Column checklist, Select All/Unselect All, "My data has headers" | A sentence + Remove/Cancel (a7.cjs) | P2 | M |
| XL-UX-108 | Phone (390 px) | Touch handles overlap cell text; command bar overflows | Excel mobile handles sit at the selection corners outside the text; command bar scrolls with a fade | Blue handle dots over "Item" and "3"; Italic…Orientation positioned off-screen (w390.png, a8.cjs overflow list) | P2 | M |
| XL-UX-109 | Accessibility | Hidden legacy toolbar and menubar are still exposed | No duplicate controls in the accessibility tree | `.toolbar` (`role=toolbar aria-label="Formatting"`) at left −99999 px is not `inert` or `aria-hidden`; `#menubar` likewise; it still contains text-glyph buttons "A▲", ".0→" (a15.cjs; a1.json control dump) | P2 | S |
| XL-UX-110 | Shortcuts | Shift+F2 opens Comments, not a Note | Shift+F2 = Insert/Edit Note | Comments side panel (k2-Shift_F2.png) | P2 | S |
| XL-UX-111 | Sheet tab colour | Only 8 fixed swatches | Full theme + standard palette + More Colors… | 8 swatches + none (ctx-all.png) | P3 | S |
| XL-UX-112 | Menus | Destructive items coloured red | Excel menus use no colour for Delete | Red "Delete 1 column/row", red "Delete" on the sheet tab (ctx-all.png) | P3 | S |
| XL-UX-113 | Colour popups | Custom section differs | Theme Colors, Standard Colors, Recent, More Colors…, (Eyedropper) | "CUSTOM #RRGGBB" input + Apply + "Pick from screen" (dd-all.png) | P3 | S |
| XL-UX-114 | Home › Merge menu | Sentence case | Merge & Center, Merge Across, Merge Cells, Unmerge Cells | "Merge & center, Merge cells, Merge across (each row), Unmerge" (dd-all.png) | P3 | S |
| XL-UX-115 | Home › Orientation menu | Different set and wording | Angle Counterclockwise, Angle Clockwise, Vertical Text, Rotate Text Up, Rotate Text Down, Format Cell Alignment | "None, Angle up (45°), Angle down (45°), Rotate up (90°), Rotate down (90°), Stacked" (dd-all.png) | P3 | S |
| XL-UX-116 | Home › Font group | Strikethrough button in the Font group | Font group row 2: B, I, U▾, Borders, Fill, Font Color (no strikethrough) | Strikethrough present (`ribbon.model.js:43`) | P3 | S |
| XL-UX-117 | Home › Font list | No Theme Fonts or Recently Used sections | Theme Fonts (Headings/Body), Recently Used, All Fonts | A plain list of every font (dd-fontlist.png) | P3 | S |
| XL-UX-118 | Home › Font size | Out-of-range size silently ignored | "The number must be between 1 and 409" | 400 → reverts to 11 with no message (a14.cjs) | P3 | S |
| XL-UX-119 | Page Layout | Print button on Page Layout | Print lives in File › Print only | `file.print` on Page Layout (`ribbon.model.js:96`) | P3 | S |
| XL-UX-120 | Data tab | Non-Excel commands on the ribbon | No "Convert Text to Numbers" (it is the error smart tag); no "Column Statistics" | Both present (`ribbon.model.js:131,141`) | P3 | S |
| XL-UX-121 | Review tab | Lock Cell / Hide Formula placed on Review | Lock Cell is under Home › Format; Hidden is in Format Cells › Protection | `ribbon.model.js:149` | P3 | S |
| XL-UX-122 | View › Zoom | 100% button uses a magnifier icon; Zoom is a menu of presets | Zoom opens the Zoom dialog (presets + Custom + Fit selection); 100% has its own icon | `L("view.zoom.100", "search", "100%")` (`ribbon.model.js:162`) | P3 | S |
| XL-UX-123 | Ribbon controls | Combo boxes 22 px tall vs 24 px small buttons | docs/95 rule 2: small controls 24 px | Font/Size/Number-format inputs h=22 (a1.json control dump) | P3 | S |
| XL-UX-124 | Search results | Plain text rows | Grouped Actions with icons, shortcuts and submenus (e.g. Borders gallery), Help, Files | "Borders" text only (misc2.png) | P3 | M |
| XL-UX-125 | Title bar | Save state wording | "Saved to this PC" / AutoSave on/off dropdown on the file name | "Saved" / "Unsaved changes" text (docname in misc.png) | P3 | S |
| XL-UX-126 | Format Cells | Dialog is small and dense | ~460×500 px, fixed layout, Sample box on every category | 560×456 box with a narrow category list (a7.cjs) | P3 | M |
| XL-UX-127 | Ctrl+Shift+F | Opens the Number tab | Opens Format Cells on the Font tab | Number tab (k2-Control_Shift_KeyF.png) | P3 | S |
| XL-UX-128 | Paste Special | Ctrl+Alt+V with an empty clipboard gives only a status message | Dialog opens (options disabled for an empty clipboard) | "clipboard is empty" (a5.cjs) | P3 | S |
| XL-UX-129 | Create Table (Ctrl+T) | Adds a style picker | "Create Table" with only range + "My table has headers" | Range, headers, Style swatches; title "Create table" (k2-Control_t.png) | P3 | S |
| XL-UX-130 | Text to Columns | One-page dialog | 3-step Convert Text to Columns Wizard (Delimited/Fixed width, delimiters + preview, column data format) | Single dialog with delimiter checkboxes, no preview (a7.cjs) | P3 | M |
| XL-UX-131 | Shortcut consistency | Redo shortcut stated differently | Ctrl+Y (Ctrl+Shift+Z also works) | Title-bar tooltip "Redo (Ctrl+Y)", legacy toolbar "Redo (Ctrl+Shift+Z)", menu model and Keyboard Shortcuts dialog "Ctrl+Shift+Z" (a1.json; dlg-help_keyboard_shortcuts.png) | P3 | S |
| XL-UX-132 | Shortcuts | Ctrl+Shift+A acts like Ctrl+A | Inserts argument names after a function name in a formula | Selects the region, active cell D5 (a5.cjs) | P3 | S |
| XL-UX-133 | Shortcuts | Ctrl+Enter / Alt+Enter in Ready move the cursor down | No movement in Ready **[unverified]** | B2 → B3 (a5.cjs) | P3 | S |
| XL-UX-134 | Shortcuts | F11 / Alt+F1 do nothing | Create a chart sheet / embedded chart from the selection | No chart (k2-F11.png, k2-Alt_F1.png) | P3 | M |
| XL-UX-135 | Shortcuts | F12 does nothing | Save As | Nothing (k2-F12.png) | P3 | S |
| XL-UX-136 | Shortcuts | Ctrl+E does nothing | Flash Fill | Nothing (k2-Control_e.png) | P3 | L |
| XL-UX-137 | Shortcuts | F8 / Shift+F8 show no mode indicator | Status bar shows "Extend Selection" / "Add to Selection" | Status unchanged (a5.cjs) | P3 | S |
| XL-UX-138 | Shortcuts | Shift+F11 appends after the last sheet | Inserts the new sheet before the active sheet | Sheet2…Sheet15 appended in order (many-sheets2.png) | P3 | S |
| XL-UX-139 | Formula bar display | Date-formatted cell shows the display text | Formula bar shows the date in the locale short form (1/3/1900) | "3-Jan-00" after Ctrl+Shift+# (a5.cjs) | P3 | S |
| XL-UX-140 | AutoFilter buttons | Drawn as a bare caret in the header text | Boxed drop-down button at the cell's right edge | Inline "▾" after the label (g-filter in g-all.png) | P3 | S |
| XL-UX-141 | Grid corner | Select-all corner drawn as a pill plus triangle | A plain grey box with a triangle in the lower right | Rounded oval + triangle at the top-left (01-default-1920.png; `editor.core.js:3457`) | P3 | S |
| XL-UX-142 | Dark mode | Explicit light header fill on a dark grid | Excel's dark grid inverts or keeps the page white per setting **[unverified]** | A1:D1 light grey band on a dark sheet (dark-dlg.png) | P3 | S |
| XL-UX-143 | About dialog | Shows the engine version and "Alpha" | Account › About Excel with a product/version line | "Engine v0.0.0 · Alpha" (a7.cjs); docs/95 rule 7 forbids version strings | P3 | S |
| XL-UX-144 | Zoom % button | Clicking 100% resets the zoom | Clicking the percentage opens the Zoom dialog | `aria-label="Zoom level, reset to 100%"` (a1.json) | P3 | S |
| XL-UX-145 | Help tab | Key and contents | KeyTip Y2 **[unverified]**; Help, Contact Support, Feedback, Show Training, What's New | Key "Y"; Keyboard Shortcuts + About (`ribbon.model.js:173`) | P3 | S |

## Differences from Excel (audit, 2026-10-17)

Row `XL-UX-169`. Owner, 2026-10-17 (docs/95 rule 9): OpenCalc is a clone of Excel
for Microsoft 365 on Windows in UI, UX and user flows. A difference may stay only
for one of two reasons. The first is a **browser limit**: something a web page
physically cannot do, named below. The second is an **own asset**: name, logo,
icons, fonts or colour. Every other shipped difference is **to convert**.

**Method.**

- **Text sweep.** `docs/14` (each Done row read), `docs/14a`, `docs/95`, `docs/96`,
  `docs/99`, the other `docs/*.md`, `webapp/*.{js,css,html}` and
  `tests/browser/*.spec.mjs` were searched for "deliberate", "divergen…",
  "differs from / unlike / instead of / where Excel", "Excel does not / has no /
  would", "kinder", "we chose", "owner decision", "our own", "rule 8",
  "omitted" and "not Excel".
- **Live run.** The editor was driven with Playwright: every ribbon tab, the
  four context menus, about 27 dialogs and the File rail. The Insert Cells
  confirmation and typing into a locked cell were both run.
- **Excel's behaviour.** Taken from the Microsoft pages linked in the table. A
  row marked **[unverified]** rests on remembered Excel behaviour, or on a
  search-result snippet rather than a fetched page.
- **Excluded.** Gaps already filed as open rows for missing features are not
  repeated. They are cited only where the shipped behaviour is itself a
  difference.

**Counts.** 6 browser limits, 5 own assets, 29 to convert. Of the 29, one
(`XL-UX-168`, dialog messages) is converted with this audit, and 8 had no row:
they are proposed as new rows N1–N8, for [14](14-EXECUTION-TRACKER.md) to number.

| Surface | What OpenCalc does | What Excel 2026 does (source) | Class | Row |
| --- | --- | --- | --- | --- |
| File › Open | Browse only, using the browser's file picker. No Recent, This PC or folder paths. | The Open page lists Recent and locations, plus Browse **[unverified]**. | **browser limit**: a page cannot list or reopen local paths. File System Access is Chromium-only and exposes no paths. | XL-UX-028 (Recent) |
| File › Save / Save As / F12 | "Save a Copy" downloads the file in a chosen format. There is no folder picker. | A Save As dialog to any path, and AutoSave **[unverified]**. | **browser limit**: a page cannot write to a path the user picks (outside Chromium's File System Access). The rail name and the mixed pane are to convert. | XL-UX-048, 095, 135 |
| Printing | The browser's print dialog; the operating system supplies the printer and its properties. | A Print pane with a printer picker, settings and a preview **[unverified]**. | **browser limit**: a page cannot list printers. The missing preview and settings pane is to convert. | XL-UX-029 |
| Insert Hyperlink: file targets | No Create New Document. Existing File is an Address box only. Only http(s) and mailto are accepted. | Create New Document; Current Folder, Browsed Pages and Recent Files; file and UNC targets ([work with links](https://support.microsoft.com/office/work-with-links-in-excel-7fc80d8d-68f9-482f-ab01-584c44d72b3e)). | **browser limit**: a page cannot create or list local files, and browsers block `file:` navigation from http(s). | XL-UX-164 |
| Clipboard | Ctrl+V is left to the browser's paste event. A copy can fall back to "copied here — the system clipboard was refused". | The native clipboard with every format **[unverified]**. | **browser limit**: the Async Clipboard API needs a permission or a user gesture and carries only HTML and plain text. The message wording is to convert. | XL-UX-070 (wording) |
| VBA | VBA is read-only in a viewer; macros are JavaScript. | Runs VBA **[unverified]**. | **browser limit**: VBA's object model reaches COM, Win32, the file system and the shell ([99](99-MACROS-AND-SCRIPTS.md)). | MACRO-05 |
| Name, logo, About, Security Notice | "OpenCalc", our logo, "OpenCalc Security Notice", and "OpenCalc" as the message-box title. | Microsoft branding. | **own asset** | — |
| Accent colour | `#0e7a55` | `#217346` / `#107C41` | **own asset** | — |
| Icon artwork | Fluent UI System Icons (MIT), message-box icons included. | Office icons. | **own asset** | — |
| Fonts | Inter for the UI and Carlito for cells, while the font box reads "Calibri". | Segoe UI; Aptos Narrow is the M365 default cell font **[unverified]**. | **own asset**: neither Aptos nor Calibri can be shipped. A font box that names Calibri while Carlito is drawn needs an owner note. | — |
| Help, templates, function descriptions | Our own Help tab content, Keyboard Shortcuts dialog and paraphrased function text. | Microsoft's text. | **own asset**: Microsoft's text is copyrighted. The Help tab's *structure* is to convert. | XL-UX-145, 158 |
| Dialog validation messages | Shown inline in the dialog: Go To, Go To Special, Insert Hyperlink, Create Table, Row Height and Column Width, New Name, Name Manager, Confirm Password, Unprotect, Goal Seek, Find & Replace, the CF formula. | A message box over the dialog. | **to convert**: **converted** under XL-UX-168 (below). | XL-UX-168 |
| Grid and command alerts | A data-validation Stop shows "Not allowed here — …" in the status bar. Warning and Information accept the value without asking (`editor.selection.js`). Typing in a locked cell gets a status-bar line, as do "cannot delete the last sheet", pivot-cell edits and formula errors. | Message boxes, and Warning asks Yes/No ([data validation](https://support.microsoft.com/office/apply-data-validation-to-cells-29fecbcc-d1b9-42c1-9d76-eff3ce5f7249)). The locked-cell box reads "The cell or chart you're trying to change is on a protected sheet…" ([protect a worksheet](https://support.microsoft.com/excel/protect-a-worksheet); wording **[unverified]**). | **to convert** | new: N2, XL-UX-059 |
| Confirmations and refusals over a dialog | Name Manager and Macros close themselves to ask "Delete …?", then reopen. A custom number format is never refused: any code is stored. | Excel asks over the dialog, which stays open, and refuses an unreadable code with "Microsoft Excel cannot use the number format you typed." **[unverified]**. | **to convert**: the second needs the engine to say which codes it cannot read. | new: N2 |
| Insert/Delete Cells (shift) | References are not rewritten. A "Formulas reference these cells" confirmation warns instead (`editor.core.js` `registerShellCommands`). | "Cell references automatically adjust to match the location of the shifted cells" ([insert cells](https://support.microsoft.com/office/95f00e00-5f58-43f5-971b-7a6550030556); the Mac page, which the [Windows page](https://support.microsoft.com/office/insert-or-delete-cells-rows-and-columns-6f40e6e4-85af-45e0-b39d-65dd504a3246) does not contradict). | **to convert** | new: N1 |
| Sort A→Z / Z→A | From one cell, sorts the whole used area from A1, not the block around the cell. A partial selection is sorted without a warning (`sortTarget`, `editor.selection.js`). | Sorts the current region. A partial selection raises the Sort Warning: "Expand the selection" or "Continue with the current selection" ([sort](https://support.microsoft.com/office/sort-data-in-a-range-or-table-62d0b95d-2a90-4610-a6ae-2e545c4a4654)). | **to convert** | new: N3 |
| Copy | Leaves out manually hidden rows and columns (`session_copy_tsv`; M3-5). | "By default, Excel copies hidden or filtered cells in addition to visible cells" ([copy visible cells only](https://support.microsoft.com/office/copy-visible-cells-only-6e3a1f01-2884-4332-b262-8b814412847e)). | **to convert** | new: N4 |
| Ribbon button labels | Off by default, behind a File › Options › Ribbon "Labels" setting (owner, 2026-10-14). | The classic ribbon labels its large buttons and has no such setting ([ribbon](https://support.microsoft.com/office/show-or-hide-the-ribbon-in-office-d946b26e-0c8c-402d-a0f7-c6efa296b527); labels **[unverified]**). | **to convert**: the 2026-10-14 decision conflicts with rule 9 and needs the owner. | new: N5 |
| Developer tab | Shown by default. | It "isn't displayed by default" ([show the Developer tab](https://support.microsoft.com/office/show-the-developer-tab-e1192344-5e56-4d45-931b-e5fd9bea2d45)). | **to convert** | new: N5 |
| Ribbon keyboard and display | No Full-screen mode. The arrow keys move only between tabs; the band is reached with Tab. | Full-screen, Show tabs only and Always show; the arrow keys move among ribbon items ([keyboard shortcuts](https://support.microsoft.com/office/keyboard-shortcuts-in-excel-1798d9d5-842a-42b8-9c99-9b7213f0040f)). | **to convert** | new: N5, XL-UX-009 |
| Ribbon content | Commands that are not built are left off rather than shown disabled, which leaves thin groups. | The full tab and group set. | **to convert** | XL-UX-038, 040–043, 087, 088, 146 |
| Non-Excel ribbon commands | Column Statistics, Convert Text to Numbers, Zero Values, Version History, Exit Sheet View, Print on Page Layout, Lock Cell on Review. | Absent, or in another place. | **to convert** | XL-UX-084, 090, 119, 120, 121 |
| Hyperlink ScreenTip and hold | The ScreenTip is an inline field. Press-and-hold to select a link cell waits a guessed 600 ms. | ScreenTip… opens its own "Set Hyperlink ScreenTip" box. The hold lasts "until the pointer becomes a cross" ([work with links](https://support.microsoft.com/office/work-with-links-in-excel-7fc80d8d-68f9-482f-ab01-584c44d72b3e)). | **to convert**: the hold time must be measured on Excel. | new: N6 |
| Hyperlink recent lists | No Browsed Pages and no recent e-mail addresses. | Both shown (same page). | **to convert**: recent web and e-mail lists are not a browser limit. | XL-UX-167 |
| Side panels instead of dialogs | CF, DV and Page Setup are 321 px side panels. Tables and charts use panels, not contextual tabs. | Modal dialogs, and the Table Design and Chart Design contextual tabs **[unverified]**. | **to convert** | XL-UX-073, 086, 089, 106, UX-SHELL-11 |
| Format Cells | Tabs run Number, Font, Alignment, Fill, Border. There is no Protection tab, and the button reads Apply. | Number, Alignment, Font, Border, Fill, Protection; OK ([KB 264372](https://support.microsoft.com/kb/264372), an older Excel page). | **to convert** | XL-UX-044 (extend with the order), 100, 126 |
| Other dialogs | Paste Special has five options. Sort is "Sort range" with three fixed levels. Create Table adds a style picker. Text to Columns is one page. Remove Duplicates is a sentence. Cell Styles is a modal. | Excel's dialogs ([sort](https://support.microsoft.com/office/sort-data-in-a-range-or-table-62d0b95d-2a90-4610-a6ae-2e545c4a4654)). | **to convert** | XL-UX-047, 074, 101, 102, 107, 129, 130 |
| Casing | Sentence case: "Paste special", "Data validation", "Conditional formatting", "Create table", "Text to columns", "Remove duplicates", "Cell styles", "Page setup", "Keyboard shortcuts". | Title Case **[unverified]**. | **to convert** | XL-UX-104, 105 |
| Context menus | The cell, header and sheet-tab menus differ in items, order and case. Delete is red. No mini toolbar and no Paste Options row. Eight tab-colour swatches. | Excel's menus **[unverified]**. | **to convert** | XL-UX-034, 055–058, 111, 112 |
| Status bar and sheet-tab row | One shared row, with "Local only · nothing uploaded". Right-click does nothing. No view buttons. | Separate rows, customised by right-click ([status bar options](https://support.microsoft.com/en-us/excel/excel-status-bar-options); search result). | **to convert** | XL-UX-030–033, 051–054 |
| File backstage | The rail is New, Open, Info, Save a Copy, Print, Share, Options. Print is a three-item list. Options is a page. | Adds Home, Export, Close and Account; Print has a preview; Options is the Excel Options dialog **[unverified]**. | **to convert** | XL-UX-028, 029, 048, 049, 050 |
| Title bar | A gear popover holds Scroll speed and an Accent picker. No AutoSave. The default name is "Untitled workbook". | No gear; AutoSave; "Book1" **[unverified]**. | **to convert**: only the fixed accent colour is an own asset. | XL-UX-095, 097, 098, 125 |
| Macros dialog | A code editor inside the dialog, with New function and an Autostart checkbox. | Macro name, Macros in and Description; Run, Step Into, Edit, Create, Delete, Options **[unverified]**. | **to convert**: the macro language itself is the VBA row above. | new: N8 |
| Find | Every match is tinted on the grid. | Find All lists the matches and nothing is tinted ([find or replace](https://support.microsoft.com/office/find-or-replace-text-and-numbers-on-a-worksheet-0e304ca5-ecef-4808-b90f-fdb42f892e90)). | **to convert** | new: N7 |
| Non-Excel grid gestures | The freeze divider can be dragged. A "Clip" overflow state exists. Ctrl+Alt+0 resets the zoom, and Ctrl+Alt+= / Ctrl+Alt+− are not bound. | Freeze is set from the menu only ([freeze panes](https://support.microsoft.com/office/freeze-panes-to-lock-rows-and-columns-dab2ffc9-020d-4026-8121-67dd25f2508f)). Text wraps or overflows ([wrap text](https://support.microsoft.com/office/wrap-text-in-a-cell-2a18cff5-ccc1-4bce-95e4-f0d4f3ff4e84)). Ctrl+Alt+= / − zoom ([keyboard shortcuts](https://support.microsoft.com/office/keyboard-shortcuts-in-excel-1798d9d5-842a-42b8-9c99-9b7213f0040f)). | **to convert** | new: N7, XL-UX-109 |
| Crash recovery | A bar above the toolbar ("deliberately not a modal", `editor.css`). | A Document Recovery pane **[unverified]**. | **to convert** | new: N8 |
| Row height, column width | Measured in pixels, with a 2,000 px ceiling. | Points and characters: 409 pt and 255 characters. | **to convert** | XL-UX-035 |
| Zoom | 25–200%. Clicking 100% resets the zoom, and Zoom is a menu of presets. | 10–400%, and the Zoom dialog. | **to convert** | XL-FEAT-095, XL-UX-122, 144 |
| Shortcuts | Shift+F2 opens Comments. Ctrl+Shift+A selects. F12 does nothing. Redo is listed as Ctrl+Shift+Z. | Shift+F2 is a Note; Ctrl+Shift+A inserts argument names; F12 is Save As; Redo is Ctrl+Y ([keyboard shortcuts](https://support.microsoft.com/office/keyboard-shortcuts-in-excel-1798d9d5-842a-42b8-9c99-9b7213f0040f)). | **to convert** | XL-UX-110, 131, 132, 135 |
| About | "Engine v0.0.0 · Alpha". | A product and version line. | **to convert** (and docs/95 rule 7) | XL-UX-143 |

**Docs that state a contract the code does not keep.** Each is tracked by the row
named, not corrected in the prose:

- docs/91 says Left and Right in the ribbon body cross group edges.
  `ribbon.js` handles arrows only on the tab list (N5).
- docs/99 says the Developer tab carries Record Macro and Convert VBA. It has
  Visual Basic and Macros (N5).
- docs/12 says the single-cell sort "expands silently … worth keeping". It
  expands to the used area, not the current region (N3).
- docs/14a M3-5 frames hidden-row skipping as Excel's behaviour (N4).
- docs/96's area table calls data validation "Full-ish … error messages and
  styles", but no alert is a message box (N2).

### Dialog messages as Excel's message box (XL-UX-168)

One component, `messageBox()` in `webapp/editor.dialogs.js`, carries every
message a dialog gives about its own input. Its title bar reads "OpenCalc"
where Excel's reads "Microsoft Excel" (own asset), and it shows Excel's
warning, information or error icon (Fluent artwork), the message and OK. OK
has the focus, and Enter, Space or Escape dismisses the box. While it is up,
nothing behind it takes a key or a click. Dismissing it returns the focus to
the field the message is about, with that field's text selected, and the
dialog keeps what was typed. `editor.message-box.spec.mjs` drives every site
through it.

| Dialog | Message | Source |
| --- | --- | --- |
| Go To; Insert Hyperlink (Place in This Document); following a link to a missing place | Reference isn't valid. | **[unverified]**: remembered; no Microsoft page quotes it |
| Go To Special | No cells were found. | **[unverified]**: remembered. It is also Excel's `SpecialCells` run-time error text. |
| Create Table | Reference isn't valid. | **[unverified]** |
| Find & Replace, nothing found | We couldn't find what you were looking for. Click Options for more ways to search. | quoted in [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5173659/excel-find-or-replace-does-not-work). OpenCalc had dropped the second sentence. |
| Replace All | All done. We made *n* replacements. (information icon) | **[unverified]** |
| Confirm Password | The confirmation password is not identical. | **[unverified]**. OpenCalc said "Confirmation password is not identical." |
| Unprotect Sheet | The password you supplied is not correct. Verify that the CAPS LOCK key is off and be sure to use the correct capitalization. | [KB 2849975](https://support.microsoft.com/kb/2849975) |
| Goal Seek | Cell must contain a formula. / Cell must contain a value. / Reference must be to a single cell. / Invalid value. | [GoalSeek method](https://learn.microsoft.com/en-us/previous-versions/office/developer/office2000/aa298303(v=office.10)) for the first three conditions; the wording is **[unverified]** |
| New Name, Name Manager | The engine's refusals, already Excel's ("The name that you entered is not valid. …") | **[unverified]** |
| Row Height, Column Width | Our own words, because the dialog measures in pixels (XL-UX-035) | — |
| Insert Hyperlink address | "Only web (http, https) and e-mail addresses can be linked." | Our own: a browser limit. Excel accepts `file:` and UNC targets. |
| Conditional formatting formula | The engine's parse refusal | Excel says "There's a problem with this formula." **[unverified]**; it converts with XL-UX-059 |

Excel's icon for each message, and whether Excel closes Go To Special and
Unprotect Sheet before raising the box, are **[unverified]**. OpenCalc keeps the
dialog open, which is the behaviour docs/95 rule 8 states.
