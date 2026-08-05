# 48 — Feature Pipeline

_Generated 2026-08-06. A dependency-ordered roadmap synthesized from the [feature/UX map](47-UX-AND-FEATURE-MAP.md) and the [design system](49-DESIGN-SYSTEM.md) assessment. Priority: P0 broken-basics · P1 core parity · P2 advanced · P3 nice-to-have. Effort: S/M/L/XL. ∥ = parallelizable as an isolated workflow agent without file conflicts._

## M1 — Transaction integrity & undo correctness
_Theme: Broken basics: data integrity_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M1-1 | Route all sheet & structural mutations through commit_edit (merge/unmerge, freeze, hide/unhide, add/rename/delete/duplicate/reorder/tab-color, resize-all) | 🔴 P0 | XL | · | — | ~10 ops currently mutate workbook_mut() directly so Ctrl+Z silently fails and the doc never dirties; this is the pervasive lib.rs foundation everything else serializes behind. |
| M1-2 | Structural insert/delete atomically shifts merges, AxisSizing widths/heights, hidden sets, and frozen counts | 🔴 P0 | L | ∥ | — | structural.rs shifts only cells+formulas, so any sheet with hidden rows/custom widths/merges/freeze silently corrupts on insert/delete; work is confined to the transaction crate, not the hot files. |
| M1-3 | Sheet rename/delete rewrites cross-sheet formula refs and triggers recalc | 🔴 P0 | L | · | M1-1 | Refs are stored as name strings resolved at eval time, so renaming a referenced sheet silently breaks every =Other!A1 to #REF!; reuses the structural rewrite machinery but touches lib.rs. |
| M1-4 | Split clear into Clear Contents (keep style) / Clear Formats / Clear All; Delete key keeps formatting | 🔴 P0 | M | · | M1-1 | ClearCell removes the whole cell so Delete wipes fill/borders/number format, contradicting every reference product; touches lib.rs + editor.js. |
| M1-5 | Batch multi-range formatting into one undo op; add undo/redo action labels; soft-cap history | 🟡 P2 | M | · | M1-1 | One bold across 3 Ctrl+click ranges is currently 3 undo steps; ergonomics + latent unbounded-Vec memory concern. |

## M2 — Fidelity & xlsx round-trip
_Theme: Broken basics: import/export fidelity_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M2-1 | Date/time number formats honor the token layout (mm-dd-yy, d-mmm, etc.) | 🔴 P0 | M | ∥ | — | Confirmed: numfmt.rs renders every date as YYYY-MM-DD regardless of code; isolated to casual-calc-layout, zero hot-file collision, so a clean parallel agent. |
| M2-2 | Show/hide gridlines flag + sheetView showGridLines round-trip | 🟠 P1 | S | · | — | Gridlines always drawn; imported hidden-gridline sheets render wrong; model+import/export+editor.js draw gate. |
| M2-3 | Group/outline model fields + parse/emit outlineLevel/collapsed/outlinePr | 🟠 P1 | L | ∥ | — | Imported outline levels are silently dropped today; landing the model+I/O first stops the fidelity loss before any UI; confined to model+import/export crates. |
| M2-4 | Round-trip indent, pattern/gradient fills, and sheetView zoomScale (preserve even before UI) | 🟠 P1 | M | ∥ | — | These attributes are dropped on load, flattening imports; preserving them in the model/import/export unblocks later UI without data loss. |

## M3 — Clipboard & data entry
_Theme: Core parity: editing_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M3-1 | Internal rich clipboard: capture value+formula+style with relative-ref shift, plus HTML/TSV OS payload | 🔴 P0 | XL | · | — | Copy serializes display text only, so copying a formula pastes its value and all formatting is lost — the single most important clipboard behavior is broken; touches lib.rs + editor.js. |
| M3-2 | True Cut: Ctrl+X, pending-move state, marching-ants, clear source on paste with ref rewrite | 🟠 P1 | M | · | M3-1 | Cut is menu-only and destroys the source immediately as value-only; needs the rich clipboard + move semantics. |
| M3-3 | Paste Special (values/formats/formulas-only, transpose, arithmetic ops) via Ctrl+Shift+V | 🟠 P1 | M | · | M3-1 | No paste-special at all; entirely built on the internal rich clipboard model. |
| M3-4 | Autofill series engine (numeric/date/month/weekday, linear & growth) + Ctrl+D/Ctrl+R + dbl-click autofill + options popup | 🟠 P1 | L | · | — | Fill only tiles source (1,2→1,2 not 3,4); the defining autofill behavior is absent; series detection in session_fill (lib.rs) + editor.js gestures. |
| M3-5 | Visible-cells-only copy (skip filtered/hidden rows) | 🟠 P1 | M | · | M3-1 | Copying a filtered range silently includes hidden rows — a data-integrity surprise with the existing filter feature. |
| M3-6 | Column-value autocomplete (ghost text) + Alt+Down pick-from-list | 🟡 P2 | M | · | — | Biggest data-entry accelerator in Excel/Sheets; currently only function names autocomplete. |

## M4 — Navigation, selection & viewport
_Theme: Core parity: navigation_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M4-1 | Editable Name Box: parse A1/range, jump+select+scroll on Enter, live 'NR x NC' during drag | 🟠 P1 | M | · | — | cell-ref is a read-only span; this is the single highest-value missing navigation feature and the parser it needs is reused by Go-To and Name Manager; touches editor.js + html. |
| M4-2 | Ctrl+G / F5 Go-To dialog | 🟡 P2 | S | · | M4-1 | Reuses the Name Box reference parser; covers the remaining go-to need. |
| M4-3 | Keyboard nav completeness: End/End-mode, Alt+PgUp/Dn, Tab-wrap + Enter-return in a selection, Ctrl+Backspace, Shift+Space/Ctrl+Space, Ctrl+PgUp/Dn sheet switch | 🟠 P1 | L | · | — | Several standard keys unhandled and data-entry ergonomics fall short of Excel; all in the editor.js keydown handler. |
| M4-4 | Header drag-select + Shift-extend + Ctrl/Cmd multi-select of rows/cols, with edge auto-scroll; factor edge-pan to also drive drag-fill and ref-drag | 🟠 P1 | L | · | — | Header selection is click-only and auto-scroll is wired only to cell-drag, so you cannot drag-select headers or fill/pick refs past the viewport — biggest Selection-domain gap. |
| M4-5 | Persist per-sheet selection & scroll position; restore on sheet switch; persist activeTab to xlsx | 🟠 P1 | M | · | — | switchSheet slams every sheet back to A1, losing context Excel preserves. |
| M4-6 | Coalesce redraws through a single requestAnimationFrame + cache measure() on scroll-only changes | 🟠 P1 | M | · | — | draw() runs synchronously inside the wheel handler re-calling measure()+WASM JSON round-trips every tick, dropping frames and contradicting the 60fps target; core editor.js render loop. |
| M4-7 | Scrollbar track-click paging, stable virtual extent, drag tooltip, role=scrollbar + arrow-key a11y; Shift+wheel horizontal fallback | 🟡 P2 | M | · | M4-6 | Thumb is drag-only and rescales as you scroll into empty space; plain-wheel users can't scroll horizontally. |
| M4-8 | Progressive Ctrl+A (data region then all); Ctrl+click toggle-off; right-click preserves multi-range selection | 🟡 P2 | M | · | — | Ctrl+A over-selects, mis-clicks can't be corrected, and right-click inside a banked range collapses the multi-range. |
| M4-9 | Draggable freeze divider + preserve scroll position on freeze change; thicker/shadowed divider | 🟡 P2 | M | · | M1-1 | Freezing snaps to A1 and the only affordance is the menu; freeze must be undoable (M1-1) first. |

## M5 — Named ranges & defined names
_Theme: Core parity: formulas infrastructure_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M5-1 | session_define_name/rename/delete/list ops (transaction-backed) + resolve names in the dependency graph | 🟠 P1 | L | · | M1-1 | Names resolve at eval but can't be created/edited/listed and blanket-dirty the graph; new lib.rs ops + eval graph resolution. |
| M5-2 | Name Manager UI + names in autocomplete + Name Box accepts defined names | 🟡 P2 | M | · | M4-1, M5-1 | Surfaces the ops from M5-1 and makes imported names discoverable and navigable. |

## M6 — Formula engine coverage & calc performance
_Theme: Core parity + performance_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M6-1 | Generate function_catalog from the dispatch table (single source of truth) | 🟠 P1 | S | · | — | Catalog is a hand-maintained duplicate in lib.rs that drifts; unifying it removes the per-function double-edit and unblocks isolated function work; touches lib.rs. |
| M6-2 | High-frequency functions: SUMIFS/COUNTIFS/AVERAGEIFS, IFS/SWITCH, IS-family + IFNA/NA, ROW/COLUMN/ROWS/COLUMNS, TEXT/TEXTJOIN, SUMPRODUCT, MEDIAN/STDEV/LARGE/SMALL/RANK | 🟠 P1 | XL | ∥ | M6-1 | ~57 of 450+ functions today; these are the most-used gaps. Once the catalog is generated, work is confined to eval/functions.rs — a clean parallel agent. |
| M6-3 | Persistent dependency graph updated on edit (not rebuilt per pass) to hit <50ms / 1M-cell recalc | 🟠 P1 | XL | ∥ | — | Graph is rebuilt by full scan of every formula per keystroke, defeating incremental recalc at the target scale; confined to eval/graph, guarded by the existing differential test. |
| M6-4 | Volatile-cell machinery + TODAY/NOW/TIME/HOUR/RAND/RANDBETWEEN/INDIRECT/OFFSET; iterative-calc option; circular-ref warning naming the cells | 🟡 P2 | L | ∥ | M6-3 | No volatile recalc trigger blocks a whole class of functions; #REF! cycles give no explanation; best built on the persistent graph. |
| M6-5 | Array/dynamic-array spill: Value::Array, eval broadcast, spill anchor/child marking, block render, #SPILL! collision | 🟡 P2 | XL | · | M6-3 | Large cross-layer effort (model+eval+layout+render+lib.rs); spill flags are dead scaffolding today; sequence after graph perf, start with SEQUENCE/UNIQUE as the thin slice. |

## M7 — Formula editing UX
_Theme: Core parity: formula authoring_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M7-1 | Shared formula-editor controller for inline editor AND formula bar (autocomplete, ref insertion, validation, edit lifecycle) | 🟠 P1 | L | · | — | Autocomplete/point-mode/validation are wired only to the inline editor; the formula bar gets none of it; refactor prerequisite for M7-2/M7-3. |
| M7-2 | Argument/signature tooltip inside parentheses with the current arg bolded | 🟠 P1 | M | · | M7-1 | Signature vanishes once the name is accepted — exactly when filling args you get zero help. |
| M7-3 | Colored reference highlighting (tokenizing overlay + on-grid ref boxes) + keyboard point mode + F4 anchor cycling + cross-sheet point mode | 🟡 P2 | L | · | M7-1 | Plain <input> can't color tokens; no visual link between refs and cells; needs the shared controller and a styled overlay. |
| M7-4 | Multi-line inline editor (textarea, Alt+Enter newline, auto-grow) + underline/strike in wrapped & merged text paths | 🟠 P1 | M | · | — | Single-line <input> makes Alt+Enter multi-line entry impossible and wrapped/merged text skips underline/strike. |
| M7-5 | Friendly error messages + per-cell error indicator & hover tooltip + trace precedents/dependents + F9/Evaluate-Formula stepper | 🟡 P2 | L | · | — | Raw dev diagnostics leak to users, error cells look like text, and formula debugging is unsupported; recalc already memoizes sub-expressions so exposing them is tractable. |

## M8 — Formatting depth
_Theme: Core parity: formatting_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M8-1 | Enhanced color popover (theme + standard grid + custom hex/RGB + recent) shared by font & fill color, with active-color repeat | 🟠 P1 | M | · | M10-1 | Only 6 fixed swatches; engine stores arbitrary RRGGBB the UI can't produce; theme swatches/recent depend on the token layer. |
| M8-2 | Full border palette (13 placements) + line-style + line-color pickers | 🟠 P1 | M | · | — | High leverage: model+render already support per-edge style/color/thickness; the gap is almost entirely UI in editor.js + menus. |
| M8-3 | Number-format toolbar: increase/decrease decimal, comma-style, currency/locale picker, custom-format dialog with preview; engine negative/zero/text sections + [color] codes | 🟠 P1 | L | · | M2-1 | Core toolbar affordances missing and negatives never render red; builds on the date-fidelity fix in the layout crate. |
| M8-4 | Editable font-size combobox + grow/shrink buttons; searchable in-face font menu; fix autofit to measure each cell's real font | 🟠 P1 | M | · | — | Fixed 7-font / 12-size selects; autofit measures a hardcoded 13px so styled columns mis-size. |
| M8-5 | Format painter (single + double-click-lock) via batched SetStyle | 🟡 P2 | M | · | M3-1 | No way to copy formatting onto a range; reuses the style-capture from the rich clipboard. |
| M8-6 | Named cell-styles registry + starter gallery + one-click Clear Formatting | 🟡 P2 | L | · | — | No reusable styles or format-only reset; new model registry + UI. |
| M8-7 | Indent increase/decrease buttons + render + round-trip | 🟡 P2 | S | · | M2-4 | Feature entirely absent; needs the indent field preserved by M2-4 plus toolbar + text-draw padding. |
| M8-8 | Alignment depth: justify + center-across-selection, visible V-align group honoring merged cells, Merge & Center / Merge Across variants + data-loss warning | 🟡 P2 | M | · | M1-1 | Only L/C/R and a single merge-all toggle; merge must be undoable (M1-1) and merged text ignores V-align. |

## M9 — Structure UX
_Theme: Core parity: structure_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M9-1 | Real row autofit + column autofit honoring each cell's font/size/family, wrap, rotation, merges; move measurement into the layout crate so editor and autofit agree | 🟠 P1 | L | · | — | Row autofit is missing (dbl-click just resets height) and column autofit clips larger fonts; measurement belongs in layout so render and fit match. |
| M9-2 | Hidden-region header markers + hover arrows + double-click-to-unhide + Unhide All | 🟠 P1 | M | · | M1-1 | Hidden rows/cols collapse to 0px with no marker so unhiding an invisible band is near impossible; hide must be undoable (M1-1) first. |
| M9-3 | Insert/delete above/below + left/right variants; toolbar + menu-bar entry; Ctrl +/- shortcuts; delete-referenced warning | 🟠 P1 | M | · | M1-2 | Right-click-only, single direction, no shortcut; layered on the corrected structural shift from M1-2. |
| M9-4 | Numeric Column-Width / Row-Height dialog + live size tooltip during drag | 🟡 P2 | S | · | — | No numeric entry and no drag readout; small editor.js + menu addition. |
| M9-5 | Outline gutter UI with +/- collapse controls + group/ungroup shortcuts (reusing hidden mechanism) | 🟡 P2 | L | · | M2-3 | Builds the collapsible-group UI on top of the outline model landed in M2-3. |
| M9-6 | Tab overflow scroll arrows + all-sheets jump menu; drop-indicator caret + reflow on tab reorder; insert-after-active + tab context 'Insert sheet' | 🟡 P2 | M | · | M1-1 | Many sheets become unreachable and reorder gives no drop cue; sheet ops should be undoable (M1-1) first. |

## M10 — Design-system adoption
_Theme: Design polish_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M10-1 | Author the semantic token layer in style.css: elevation/surface/text/border/accent/state/danger/shadow/spacing/radius/typography, fully defined for light AND dark in both media-query and [data-theme] blocks | 🟠 P1 | L | ∥ | — | Fixes the confirmed accent-in-dark bug and the color-mix selection-tint fragility; a single self-contained style.css layer that every component then references — the foundation for the whole milestone. |
| M10-2 | Migrate chrome components (editor.css) to tokens: elevation hierarchy, unified single hover overlay, focus-visible on every control, danger/shadow/spacing/radius/control-height scales | 🟠 P1 | L | ∥ | M10-1 | Resolves the hover contradiction, four-shadow drift, absent focus rings, and ad-hoc scales; confined to editor.css so it parallelizes against Rust work (but not against other editor.css edits). |
| M10-3 | Canvas token adoption in editor.js: read grid font-size/header/selection-tint tokens via readColors, color-mix selection fill, bold weight 700, tokenized header font | 🟠 P1 | M | · | M10-1 | The canvas hardcodes 12px/13px fonts and builds tint by hex string concat; must read the same tokens as chrome — but lives in editor.js so it collides with all other editor.js work. |
| M10-4 | Accessibility floor: visible canvas focus ring, scrollbar role/ARIA, reduced-motion, mixed/indeterminate toggle state, non-color-only signals | 🟡 P2 | M | · | M10-2 | Focus is suppressed everywhere and toggles show no mixed state; completes the design-system a11y contract. |

## M11 — Advanced & nice-to-have
_Theme: Advanced parity_

| ID | Item | Pri | Effort | ∥ | Depends on | Rationale |
|---|---|---|---|---|---|---|
| M11-1 | Find & Replace depth: Replace-next, match-entire-cell, regex/wildcards, search formula text, all-sheets scope, canvas highlight of all matches | 🟡 P2 | L | · | — | Active-sheet substring-over-display-text only; can't find/replace formula text or numbers and shows no all-match highlight. |
| M11-2 | Scroll polish: momentum/inertia, eased deceleration, animated ensureVisible for large jumps, scroll tooltip | ⚪ P3 | M | · | M4-6 | Nice-to-have fluidity on top of the rAF redraw loop. |
| M11-3 | R1C1 reference style: formula-crate parse/format + workbook reference-style preference flipping headers, name box, formula rendering | ⚪ P3 | L | ∥ | — | Niche cross-layer feature explicitly deferred in the formula crate; the parse/format half is isolated to that crate. |
| M11-4 | Split panes; 3-D references (Sheet1:Sheet3!A1); external-workbook references | ⚪ P3 | XL | · | — | Adjacent-expectation power features, lower frequency; backlog until core parity lands. |

## Sequencing notes

- HARD CONSTRAINT — two monolithic hot files gate all parallelism. webapp/editor.js (2082 lines) and crates/casual-calc-wasm/src/lib.rs (2092 lines) are each a single file touched by nearly every editor-facing feature and every engine binding. Two agents both editing editor.js, or both editing lib.rs, WILL conflict. 'parallelizable: true' is therefore reserved for work confined to a different Rust crate (casual-calc-layout, -eval, -transaction, -model, -import/-export) or a standalone stylesheet (style.css). Everything marked false collides on one of the two hot files and must be serialized within its lane.
- START HERE, SERIAL: M1-1 (route all mutations through commit_edit) is the keystone. It rewrites ~10 ops across lib.rs and unblocks undoable freeze (M4-9), sheet ops (M9-6), merge variants (M8-8), defined-name ops (M5-1), hidden markers (M9-2). Because it churns lib.rs pervasively, land it before any other lib.rs work starts. M1-3 (rename ref-rewrite) and M1-4 (Delete-keeps-format) chain directly off it.
- PARALLEL BATCH A (isolated Rust crates, can run concurrently with M1-1 and with each other — no hot-file overlap): M1-2 structural metadata shift (transaction crate), M2-1 date numfmt + M2-3 outline I/O + M2-4 attribute round-trip (layout/import/export/model), M6-3 persistent dependency graph (eval/graph), and M10-1 token layer (style.css). These five are the cleanest simultaneous agents in the whole plan.
- PARALLEL BATCH B (after their isolated prereqs): M6-2 high-frequency functions becomes a clean parallel agent ONLY after M6-1 generates the catalog from the dispatch table — otherwise every function needs a second edit in lib.rs's duplicate catalog and collides. M6-4 volatile/iterative and M6-5 spill both build on the M6-3 graph; M6-5 is cross-layer (touches lib.rs + render) so it re-enters the serial editor lane.
- EDITOR.JS LANE IS INHERENTLY SERIAL. M3 (clipboard), M4 (nav/selection/viewport), M7 (formula UX), M8 (formatting UI), M9 (structure UX), and M10-3 (canvas tokens) all live in editor.js and cannot be split across parallel agents without conflicts. Sequence them by value: within M4 do M4-1 Name Box first (its A1 parser feeds M4-2 Go-To and M5-2 Name Manager) and M4-6 rAF redraw early (M4-7 scrollbars and M11-2 momentum depend on it). M7-1 shared controller must precede M7-2/M7-3.
- CLIPBOARD IS A SPINE. M3-1 internal rich clipboard unblocks M3-2 cut, M3-3 paste-special, M3-5 visible-only copy, and the style-capture reused by M8-5 format painter. Prioritize it as the first M3 item.
- DESIGN SYSTEM ORDERING: M10-1 (style.css tokens, parallel) is the root. M10-2 (editor.css migration) can run parallel to Rust work but not to other editor.css edits. M10-3 (canvas tokens) rejoins the serial editor.js lane. Gate M8-1 (color popover: theme swatches + recent colors) on M10-1 so it consumes real tokens rather than re-hardcoding palettes. M10-4 a11y closes the milestone.
- CROSS-MILESTONE GATES to respect: M8-3 number-format UI after M2-1 date fix; M8-7 indent after M2-4 round-trip; M9-3 insert/delete variants after M1-2 shift; M9-5 outline gutter after M2-3 model; M9-2 hidden markers + M4-9 freeze drag + M8-8 merge variants + M9-6 tab overflow all after M1-1; M5-2 Name Manager after M4-1 + M5-1; M11-2 momentum after M4-6.
- RECOMMENDED WAVE PLAN: Wave 1 = M1-1 (serial) alongside Batch A (5 parallel isolated-crate agents). Wave 2 = M1-3/M1-4 then the M3 clipboard spine and M4-1/M4-6 in the editor lane, with M6-1→M6-2 and M6-3→M6-4 continuing in the Rust lanes and M10-1→M10-2 in the CSS lane. Wave 3 = remaining M4/M7/M8/M9 editor work (serial, value-ordered) plus M6-5 spill and M5. Wave 4 = M11 advanced/nice-to-have.
- STALE-TRACKER FIXUP (do alongside M6): docs/45 and docs/46 claim 'full recalc only' and '8 functions' — both verified false (incremental recalc shipped; ~57 functions exist). Update CP-073/CP-080 so roadmap sizing isn't wrong, and reflect M6-3 as the perf (not correctness) work item.
