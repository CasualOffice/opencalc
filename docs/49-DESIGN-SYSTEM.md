# 49 — Design System

_Generated 2026-08-06. Two parts: the **current** de-facto design language extracted from `webapp/editor.css` / `editor.html`, and the **target** system to converge on._

> **Part A is a snapshot of 2026-08-06 and has not been regenerated since**, so
> read it as the state on that date rather than as the tree. It was accurate
> when written and nothing re-derives it — the same shape as `48`, whose own
> audit found 33 of 35 items already built. What is checked continuously is
> narrower and lives in code: `tools/check-theme-tokens.py` asserts every
> advertised `--oc-*` token is honoured.
>
> **Adoption was tracked as `M10` in [48](48-FEATURE-PIPELINE.md)**, whose
> banner records where it got to: `M10-1` closed as `UX-TOKEN-01` (24 semantic
> colour tokens, plus the elevation and surface layer that was the gap).
> `DOC-034` is the open question about the token *names*: `55` §1 argued for
> AG Grid-style typed suffixes and called the rename *"cheap now and impossible
> after the first release"*, and the release has happened.

## Part A — Current design language (as-built)

**Colors**

- `LIGHT (style.css:1-9): --bg #ffffff`
- `--fg #1a1c20`
- `--muted #5a6472`
- `--accent #2f6df6 (user-overridable via settings: green #16a34a, pink #db2777, amber #f59e0b, violet #8b5cf6; persisted localStorage oc-accent)`
- `--accent-ink #ffffff (never redefined for dark)`
- `--card #f6f8fb`
- `--border #e2e8f0`
- `DARK @media prefers-color-scheme (style.css:11-20): --bg #0f1216`
- `--fg #e7ebf0`
- `--muted #9aa6b4`
- `--accent #5b8bff (ONLY in media block; missing from data-theme=dark)`
- `--card #171b21`
- `--border #262c34`
- `MANUAL data-theme=dark/light (style.css:23-36): same bg/fg/muted/card/border, but --accent and --accent-ink intentionally-or-accidentally omitted`
- `CANVAS-derived (editor.js:61-70): bg=--bg, fg=--fg, muted=--muted, grid=--border, headerBg=--card, accent=--accent, sel=accent+'22' (~13% alpha tint for selection & header band)`
- `Hardcoded reds (no token): danger #e5484d (editor.css:111); error/invalid/err #e53935 (editor.css:167,195,196)`
- `Hardcoded on-accent text #fff (editor.css:182,191) instead of --accent-ink`
- `Font-color swatch palette: #000000 #d92d20 #1570ef #12b76a #7a5af8 #ffffff (editor.html:118-124)`
- `Fill-color swatch palette: #fff3b0 #d1f0d6 #ffd6e0 #d6e4ff #fed7aa #e9d5ff (editor.html:132-138)`
- `Tab-color Material palette: #E53935 #FB8C00 #FDD835 #43A047 #1E88E5 #5E35B1 #8E24AA #546E7A (editor.js:1507)`
- `Cell fill/text/border colors are engine-supplied hex data ('#'+it.bg / it.fc / border spec), not theme tokens (editor.js:497,525,562,281)`

**Typography**

- `--mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace (style.css:9) - cell-ref, formula input, status, stats, find-count, range-val, inline editor`
- `Chrome UI font (untokenized): 16px/1.6 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif (style.css:41)`
- `Canvas header font hardcoded: '12px system-ui, sans-serif' (editor.js:697)`
- `Cell font (editor.js:131-136): 'system-ui, sans-serif' default or cell family; italic prefix; bold => weight 600; pt->px via round(fs*4/3); default 13px`
- `UI font sizes (hardcoded, no scale): 11px menu-label; 12px sel-stats/find-count/range-val/ac-sig/find-case; 13px toolbar buttons/selects/menu items/tabs/cell-ref/filter/find-field/ac-name; 14px formula input; 15px brand; 16px tb-icon glyph`
- `Font weights mixed: 500 (popmenu/chip) / 600 (buttons/tabs/cell-ref/labels/bold-cell) / 700 (brand, ac-name) - no weight tokens`
- `letter-spacing .04em only on .menu-label + .badge uppercase labels`

**Spacing**

- `No spacing tokens - all literal px`
- `gaps: 2,3,4,6,8,10,12,14 (tb-group 4, toolbar 6, header 10, formula/settings 8/12/14)`
- `paddings: header 8/12, toolbar 6/12, buttons 6/12, popmenu 6, menu item 8/10, filter item 4/8, settings-panel 14, swatch-menu 8`
- `control heights: 34px (tb-btn/tb-select/tb-icon), 28px (find controls/sheet-add), 24px scrollbar top band, 22/24px swatch buttons, 18px small swatch`
- `icon sizes: .icon 18px, .icon-sm 13px (editor.css:38-39); scrollbar thumb 8px thick within 14px band`
- `canvas: HW/HH headers, RESIZE_GRAB 5px, AUTOSCROLL_EDGE 28px, MIN_LINE 8 (editor.js:96-97,856)`

**Radii**

- `No radius tokens - literal px`
- `5px (.sheet-rename, .filter-item)`
- `6px (popmenu button, swatch-menu button, sheet-tab top corners '6 6 0 0', sheet-add, filter-foot button)`
- `7px (scrollbar thumb, find-field/find-btn/find-action)`
- `8px (tb-select, tb-icon, toolbar button, cell-ref, formula input, ac-menu, set-row select)`
- `10px (popmenu, find-bar)`
- `12px (settings-panel, style.css .feature)`
- `14px (style.css .card:79)`
- `50% (circular swatches editor.css:70,162)`
- `999px (pills/badges style.css:57,101)`

**Shadows**

- `No shadow/elevation tokens - four ad hoc values, all pure-black rgba, not dark-adjusted:`
- `settings-panel: 0 12px 30px rgba(0,0,0,0.25) (editor.css:56)`
- `popmenu: 0 12px 30px rgba(0,0,0,0.28) (editor.css:86)`
- `find-bar: 0 8px 24px rgba(0,0,0,0.22) (editor.css:208)`
- `ac-menu: 0 8px 28px rgba(0,0,0,0.18) (editor.css:187)`
- `canvas fill-handle: 1px --bg stroke ring around a 6px accent square (editor.js:665-669) - only in-grid 'elevation'`
- `focus/selection rings: 2px --accent inline-edit border, 2px accent selection range border, swatch :hover 2px accent outline offset 1px`

### Components (as-built)

- Toolbar icon button (.tb-btn) -> editor.css:38-46 (18px .icon glyph, 34x34), variants: .tb-toggle (aria-pressed=true tint editor.css:98-101), .tb-file (label+hidden input :74-96), :disabled (color:--muted :46). Markup editor.html:48-227
- Text/pill button (.toolbar button, .tb-file) -> editor.css:74-79 (padding 6/12, radius 8, weight 600), states: :hover border-accent :79, :disabled opacity .45 :95
- Settings gear button (.tb-icon) -> editor.css:49-52 (34x34 full button; NOTE duplicate/earlier .tb-icon helper at :40); opens .settings-panel editor.css:53-59, markup editor.html:12-43
- Settings panel controls -> editor.css:60-73: .set-row select, .range-wrap input[type=range] (accent-color:--accent :66), .range-val (mono), .swatches accent picker (22px circles, aria-current border :73)
- Popmenu (dropdown) (.popmenu / .menu-wrap / .menu-sep) -> editor.css:81-97,112 (min-width 178, radius 10, shadow .28), button states :hover bg--bg+border :94; .danger color #e5484d :111. HTML menus editor.html:61-223 (save/valign/numfmt/border/freeze/sort)
- Swatch color menu (.swatch-menu / .no-fill) -> editor.css:103-110 (24px squares, :hover outline accent), font-color+fill-color pickers editor.html:117-140 with their own hardcoded hex palettes
- Context menu (.ctx-menu, position:fixed) -> editor.css:113 + reuses .popmenu; built in JS: sheetMenu editor.js:1475-1531, cellMenu editor.js:1552+; positioned/flip via positionMenu editor.js:1534-1544
- Tab-color swatch strip (.swatch-row / .swatch / .swatch-none / .menu-label) -> editor.css:158-168 (18px circles, rgba(0,0,0,.2) border, :hover outline accent); Material palette built in JS editor.js:1500-1520
- Filter dropdown (.filter-menu/.filter-list/.filter-item/.filter-all/.filter-foot/.filter-clear/.filter-apply) -> editor.css:169-182 (apply uses hardcoded #fff + --accent bg); DOM built in JS editor.js:1095-1155; states :hover bg--card, checkbox accent-color--accent
- Sheet tabs (.sheet-tab / .active / .dragging / .colored / .sheet-add / .sheet-rename / .sheet-tabs) -> editor.css:138-201; --tab-color per-tab set inline in JS editor.js:1394; renderTabs editor.js:1383-1424; states hover/active/dragging/colored(+active inset shadow)
- Custom scrollbars (.scrollbar/.vscroll/.hscroll/.thumb) -> editor.css:228-235 (thumb var(--muted) opacity .35, :hover/.drag .62); geometry in JS updateScrollbars editor.js:296-322
- Inline cell editor (.inline-edit / .invalid) -> editor.css:237-241 (2px --accent border, font 13px var(--mono)); .invalid red #e53935 :195; positioned/sized in JS startInline editor.js:1259-1273, invalid toggled editor.js:911,1281,1874
- Formula autocomplete (.ac-menu/.ac-item/.ac-name/.ac-sig) -> editor.css:183-193 (own shadow .18); .ac-item.active bg--accent + hardcoded #fff text; rendered in JS renderAutocomplete editor.js:1316-1329
- Formula bar (.formula-bar / .cell-ref / input) -> editor.css:121-132 (cell-ref mono, input mono 14px), markup editor.html:230-233
- Find & replace bar (.find-bar/.find-field/.find-count/.find-btn/.find-action/.find-case) -> editor.css:205-226 (own shadow .22); markup editor.html:236-251; matches move selection only, no canvas highlight
- Status + selection stats (.tb-status / .sel-stats / .sel-stats b) -> editor.css:119,142-146 (mono muted); .err red #e53935 :196; updateStats editor.js:745-765
- App header + toolbar shell (.app-header/.toolbar/.tb-group/.tb-sep/.tb-brand) -> editor.css:1-37,97; markup editor.html:11-46,48
- Canvas grid render (headers, gridlines, selection tint, cell fills/text, borders, merges, fill handle, freeze divider) -> all in JS draw() editor.js:384-738; colors read from CSS vars via readColors editor.js:61-70; header font hardcoded '12px system-ui' editor.js:697

### Interaction states covered

- hover: .tb-select (border-accent editor.css:34), toolbar button/.tb-file (:79), popmenu button (bg--bg :94), swatch-menu button (outline accent :109), swatch-row .swatch (outline accent :164), .sheet-tab (color--fg :152), .sheet-add (:201), .filter-item (bg--card :176), .find-btn (:220), .find-action (:225), scrollbar .thumb (opacity .62 :233). No hover rule on .tb-icon gear.
- active/pressed: .tb-toggle[aria-pressed=true] accent tint+border editor.css:98-101 (bold/italic/underline/strike/align/wrap toggles synced in refreshFormulaBar editor.js:774-782); .sheet-tab.active editor.css:153; .ac-item.active editor.css:190; scrollbar .thumb.drag editor.css:233
- selected/current: .swatches button[aria-current=true] border--fg editor.css:73; .sheet-tab.active + .colored.active inset shadow editor.css:153,157; canvas selection tint colors.sel + 2px accent range border + accent fill handle editor.js:435,626-645,665
- disabled: .toolbar .tb-btn:disabled color--muted editor.css:46 (undo/redo icon buttons) BUT .toolbar button:disabled opacity .45 editor.css:95 -> two different disabled treatments; disabled toggled in JS editor.js:770-771
- focus: NONE -> no :focus or :focus-visible rule anywhere; outline:none on .inline-edit editor.css:240, .sheet-rename :117, #grid :236; native focus rings suppressed on selects/inputs/buttons -> keyboard focus invisible (a11y gap)
- dragging: .sheet-tab.dragging opacity .45 editor.css:154 (tab reorder editor.js:1404-1407); drag-fill preview dashed accent outline on canvas editor.js:648-656

### Inconsistencies & gaps

- ⚠️ THEME/ACCENT BUG: --accent is set in @media(prefers-color-scheme:dark) (style.css:16 #5b8bff) but NOT in the manual :root[data-theme=dark] block (style.css:30-36). A light-OS user who picks Dark keeps the light accent #2f6df6 on dark surfaces. --accent-ink is never redefined for dark at all (only style.css:6).
- ⚠️ Selection tint built by string concatenation: sel = (--accent hex) + '22' (editor.js:69). Works only because every accent is a 6-digit hex; an rgb()/named/8-digit accent would silently produce an invalid color. Should be color-mix/rgba.
- ⚠️ Two different 'danger/error' reds, no token: #e5484d (.popmenu button.danger editor.css:111) vs #e53935 (.inline-edit.invalid :195, .err/#tb-status .err :196, .swatch-none gradient :167). No --danger/--error variable.
- ⚠️ Hardcoded #fff for on-accent text instead of var(--accent-ink): .filter-apply (editor.css:182) and .ac-item.active .ac-name/.ac-sig (editor.css:191) -> bypasses the existing token.
- ⚠️ Four unrelated, non-tokenized color palettes: accent swatches (editor.html:35-39), font-color (editor.html:118-124), fill-color pastels (editor.html:132-138), tab-color Material set (editor.js:1507). No shared scale; fill pastels are hardcoded light values that read poorly in dark (they are cell data, never theme-adjusted).
- ⚠️ Four different elevation shadows for the same 'floating surface' role, all pure-black rgba, not dark-aware: .settings-panel 0 12px 30px/.25 (:56), .popmenu 0 12px 30px/.28 (:86), .find-bar 0 8px 24px/.22 (:208), .ac-menu 0 8px 28px/.18 (:187).
- ⚠️ No tokens for spacing/radii/typography -> all hardcoded. Radii span 5,6,7,8,10,12,14px + 50%/999px. Font-sizes ad hoc 11,12,13,14,15,16px. Gaps/paddings 2/3/4/6/8/10/12/14/16 with no scale.
- ⚠️ Three font stacks in play: chrome UI '-apple-system,BlinkMacSystemFont,Segoe UI,Roboto' (style.css:41), canvas headers hardcoded '12px system-ui' (editor.js:697), cell text 'system-ui,sans-serif'/var(--mono) (editor.js:134); no --font-ui token.
- ⚠️ Icon stroke-weight mismatch: most SVGs stroke-width=2, Bold 2.4 (editor.html:99), find prev/next/close 2.2 (editor.html:240,243,249); only two icon sizes 18px .icon / 13px .icon-sm (editor.css:38-39), so find/sheet-add glyphs sit at 13 beside 18px toolbar icons.
- ⚠️ Inverted/near-invisible hover between menu types: .popmenu button hovers to var(--bg) (editor.css:94) but .filter-item inside a .popmenu (bg var(--card)) hovers to var(--card) (:176) -> filter rows get almost no hover contrast.
- ⚠️ Duplicate/conflicting .tb-icon rule: inline-flex helper at editor.css:40 then fully redefined as a 34px bordered button at :49-52; the first is effectively dead for the settings button.
- ⚠️ Native accent-color applied inconsistently: var(--accent) on range slider (editor.css:66) and .filter-item checkbox (:177) but NOT on the .find-case 'Aa' checkbox (editor.html:247) or the theme <select>.
- ⚠️ Bold cells render at font-weight 600 (editor.js:132) while brand/ac-name use 700 and Excel bold is 700; weights 500/600/700 mixed with no scale, so canvas bold looks lighter than a real bold.
- ⚠️ Focus-visible styling entirely absent (see interaction_states) -> every control lacks a keyboard focus indicator; outline:none actively removed on inline editor and canvas.
- ⚠️ Find/replace has no all-matches highlight on canvas -> draw() paints only the single selection tint (editor.js:435); matches are navigated by moving selection, unlike Excel/Sheets which tint every match.
- ⚠️ Hardcoded rgba(0,0,0,.2) border on .swatch-row .swatch (editor.css:162) and light-only #e53935 diagonal in .swatch-none (:167) -> not tokenized, no dark variant.
- ⚠️ Floating-menu vertical offsets hardcoded (settings-panel top:42px :54, popmenu top:40px :83) rather than anchored -> brittle if header/toolbar height changes.

## Part B — Target design system

### Principles

- Single source of truth spanning chrome AND canvas. The grid is drawn by editor.js reading CSS variables, so tokens are not just for DOM — every gridline, header, selection tint, and cell/header font size is a named token the canvas reads. No component (CSS or canvas) may contain a raw hex or magic px; it references a token.
- Semantic tokens over primitives. Components reference role tokens (`--surface-raised`, `--text-secondary`, `--control-h-md`, `--radius-md`), never a palette scale directly. Only the `:root` blocks map roles to concrete values, and only there does a value differ between light and dark. Swapping a theme or the accent touches one layer.
- A real elevation hierarchy replaces the overloaded bg/card pair. Four explicit layers — canvas (grid/page), chrome (bars), sunken (inputs), raised (menus/dialogs) — each a fixed triple of surface + border + shadow. This removes today's contradiction where menus and filters disagree on which color means 'hover' vs 'surface'.
- Every interactive element carries the full state set — rest, hover, active/pressed, focus-visible, selected, disabled — driven by shared state tokens, so a toolbar icon button, a menu item, a sheet tab, and a filter row all behave identically. Focus-visible is always shown (keyboard parity), never suppressed.
- Theme parity by construction. Every token is defined for light and dark in both the `prefers-color-scheme` media block and the `[data-theme]` override; the manual toggle always wins. Contrast targets hold in both themes: WCAG AA (4.5:1) for text, 3:1 for gridlines/borders/icons and focus rings.
- Spreadsheet density with an 8px rhythm. A compact scale (2/4/6/8/12/16/20/24) and a single control-height scale (28/32/36) keep the chrome tight like Excel/Sheets while staying consistent; the accent is user-swappable through one variable with all tints derived via `color-mix`.
- Accessibility floors are non-negotiable: hit targets >= 28px, a visible focus ring on every focusable (including the canvas), reduced-motion respected, and color never the sole signal (selected menu items also get a check, toggles a filled state).

### Canonical tokens

- `--surface-canvas: #ffffff (light) / #0f1216 (dark) — the grid drawing surface and page base; canvas reads this as `bg` (replaces overloaded --bg).`
- `--surface-chrome: #f7f9fc / #14181e — toolbar, app-header, formula bar, bottom bar; distinguishes bars from both the white grid and white menus.`
- `--surface-sunken: #eef2f8 / #0b0e12 — inset inputs (formula input, find fields, inline-edit background); reads as recessed.`
- `--surface-raised: #ffffff / #1b2027 — menus, popovers, autocomplete, dialogs, the cell-ref chip; pairs with shadow-2/3 (replaces --card as menu surface).`
- `--surface-scrim: rgba(15,18,22,.32) / rgba(0,0,0,.55) — modal-dialog backdrop.`
- `--border-subtle: #eef2f7 / #20262e — gridlines, in-chrome dividers, menu separators (canvas reads as `grid`).`
- `--border-default: #d8dfe8 / #2b323b — control outlines, bar separators, resting menu/tab borders (maps current --border e2e8f0/262c34).`
- `--border-strong: #b7c1cd / #3a424c — hovered controls, freeze-pane dividers, header gridline emphasis.`
- `--text-primary: #1a1c20 / #e7ebf0 — default text and icons; canvas `fg`.`
- `--text-secondary: #5a6472 / #9aa6b4 — muted labels, status, counts, header cells; canvas `muted` (maps current --muted).`
- `--text-disabled: #a2acba / #5b6675 — disabled control text/icons (replaces ad-hoc opacity:.45).`
- `--text-on-accent: #ffffff / #ffffff — text/icon on any accent fill; replaces the hardcoded #fff in .ac-item.active and .filter-apply.`
- `--accent: #2f6df6 / #5b8bff — THE single user-swappable variable (JS setProperty). All accent tints derive from it.`
- `--accent-hover: color-mix(in srgb, var(--accent) 88%, #000) / color-mix(in srgb, var(--accent) 82%, #fff) — hovered accent fills (primary buttons).`
- `--accent-soft: color-mix(in srgb, var(--accent) 16%, transparent) — pressed toggle background (unifies the .tb-toggle 18% and selected-tab tint).`
- `--accent-selection: color-mix(in srgb, var(--accent) 13%, transparent) — cell/range selection fill; canvas reads this instead of computing `accent+"22"`.`
- `--state-hover: color-mix(in srgb, var(--text-primary) 6%, transparent) — the ONE hover overlay for icon buttons, menu items, filter rows, tabs (resolves the bg-vs-card hover contradiction).`
- `--state-active: color-mix(in srgb, var(--text-primary) 12%, transparent) — pressed/active overlay.`
- `--focus-ring: color-mix(in srgb, var(--accent) 45%, transparent) — used as the 3px focus halo color.`
- `--danger: #d92d20 / #f0625a — all error/destructive text and outlines (unifies #e5484d and the two #e53935).`
- `--danger-soft: color-mix(in srgb, var(--danger) 22%, transparent) — invalid-formula glow (replaces hardcoded rgba(229,57,53,.25)).`
- `--success: #12b76a / #3ddc97 — future validation/confirm states.`
- `--warning: #f59e0b / #fbbf5a — future caution states.`
- `--grid-header-bg: #f5f7fa / #161b21 — row/column header strips (canvas `headerBg`, was --card).`
- `--grid-header-active: var(--accent) — header text/tint for a selected row/column.`
- `--grid-active-cell: var(--accent) — 2px active-cell border and inline-edit border.`
- `--grid-frozen-divider: var(--border-strong) — freeze-pane split lines (heavier than a gridline).`
- `--grid-cell-font-size: 13px — canvas default cell text size (replaces hardcoded `13px system-ui`).`
- `--grid-header-font-size: 12px — canvas header text size (replaces hardcoded `12px system-ui`).`
- `--font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif — chrome + canvas UI text (canvas headers/cells align to this).`
- `--font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace — formula input, cell-ref chip, counts (current --mono).`
- `--text-xs: 11px / line-height 1.4 — uppercase menu-labels, captions.`
- `--text-sm: 12px / 1.4 — status, counts, sel-stats, header cells.`
- `--text-md: 13px / 1.5 — dominant control and menu text.`
- `--text-lg: 14px / 1.5 — formula input.`
- `--text-xl: 15px / 1.3 — brand wordmark.`
- `--weight-medium: 500, --weight-semibold: 600, --weight-bold: 700 — the three non-regular weights actually used.`
- `--space-1: 2px, --space-2: 4px, --space-3: 6px, --space-4: 8px, --space-5: 10px, --space-6: 12px, --space-8: 16px, --space-10: 20px, --space-12: 24px — 8px-based spacing scale (with 2/4/6 for dense chrome).`
- `--radius-xs: 4px (swatch cells), --radius-sm: 6px (menu items, tab tops, small buttons), --radius-md: 8px (dominant: buttons/selects/inputs), --radius-lg: 10px (popovers, find bar), --radius-xl: 12px (dialogs, settings panel), --radius-pill: 999px, --radius-round: 50% — collapses today's 5/6/7/8/10/12/14 into a scale.`
- `--shadow-1: 0 1px 2px rgba(16,24,40,.06), 0 1px 3px rgba(16,24,40,.10) / 0 1px 2px rgba(0,0,0,.4) — subtle resting lift.`
- `--shadow-2: 0 8px 24px rgba(16,24,40,.14) / 0 8px 24px rgba(0,0,0,.45) — menus, popovers, autocomplete, find bar (unifies today's 0 8px 24–28px and 0 12px 30px .25–.28).`
- `--shadow-3: 0 16px 40px rgba(16,24,40,.22) / 0 16px 40px rgba(0,0,0,.6) — dialogs and the settings panel.`
- `--shadow-focus: 0 0 0 3px var(--focus-ring) — the single focus halo applied via box-shadow.`
- `--control-h-sm: 28px — find bar fields/buttons, sheet-add, sheet tabs.`
- `--control-h-md: 32px — standard toolbar icon buttons and selects (tighten from 34px for modern density).`
- `--control-h-lg: 36px — formula bar row.`
- `--icon-size: 18px, --icon-size-sm: 13px — the two icon sizes in use.`
- `--scrollbar-track: 14px, --scrollbar-thumb: 8px — overlay scrollbar geometry.`
- `--scrollbar-thumb-color: color-mix(in srgb, var(--text-secondary) 40%, transparent), --scrollbar-thumb-hover: color-mix(in srgb, var(--text-secondary) 66%, transparent) — replaces the opacity:.35/.62 approach with themed colors.`
- `--z-grid: 1, --z-scrollbar: 6, --z-overlay: 20 (find/settings), --z-menu: 30, --z-autocomplete: 40, --z-scrim: 90, --z-dialog: 100 — formalizes today's ad-hoc 6/20/30/40.`
- `--duration-fast: 120ms, --duration-med: 180ms, --ease-standard: cubic-bezier(.2,0,0,1) — motion for hovers, menu open/close (respect prefers-reduced-motion).`

### Component specs

- Toolbar — Container: --surface-chrome, 1px --border-default bottom, padding --space-3 --space-6, inter-group gap --space-3, intra-group gap --space-2; separator (.tb-sep) 1px x 20px --border-default, margin 0 --space-1. Icon button (.tb-btn): --control-h-md square (32x32), --radius-md, transparent surface/border at rest, 18px icon in --text-primary. States — hover: --state-hover fill; active: --state-active fill; focus-visible: --shadow-focus (no border shift); disabled: --text-disabled, no hover; toggle pressed (aria-pressed=true): --accent-soft fill + --accent icon (no border tint). Selects (.tb-select): --control-h-md, --radius-md, --surface-raised, 1px --border-default, --text-md; hover --border-strong; focus --shadow-focus; size variant width 58px. Every target >= 28px.
- Menus/popovers (popmenu, ctx-menu, valign/numfmt/border/freeze/sort/save + swatch/filter variants) — Elevation-2: --surface-raised + 1px --border-default + --shadow-2, --radius-lg, padding --space-2, min-width 180px; ctx-menu position:fixed. Item: full-width, left-aligned, padding --space-3 --space-4, --radius-sm, --text-md/--weight-medium, icon+label gap --space-3. States — rest transparent; hover --state-hover (NO border swap — replaces the current bg+border hover); active --state-active; focus-visible --shadow-focus; selected/current value: --accent text + leading check glyph; danger item: --danger text, hover color-mix(--danger 10%, transparent). Label (.menu-label): --text-xs uppercase, letter-spacing .04em, --text-secondary, padding --space-1 --space-4. Separator: 1px --border-subtle, margin --space-2. Swatch cell: 24x24 --radius-xs (menu) or 18–22 --radius-pill (tab/accent), 1px --border-default; hover outline 2px --accent offset 1px; selected: 2px ring in --accent (or --text-primary for accent picker). Open/close: fade + 2px translate over --duration-fast.
- Sheet tabs (sheet-tabs / sheet-tab) — Bar: --surface-chrome, 1px --border-default top, padding --space-2 --space-4, gap --space-1, overflow-x auto with thin scrollbar. Tab: content height ~--control-h-sm, padding --space-2 --space-5, --radius-sm on top corners only, --text-md/--weight-semibold. States — rest: --text-secondary, transparent; hover: --text-primary + --state-hover; active/selected: --surface-canvas fill + --text-primary + 1px --border-default (top/sides) reading as connected to the grid; focus-visible: --shadow-focus; dragging: opacity .45. Colored tab: 3px bottom stripe in --tab-color; active-colored adds inset 0 -3px 0 --tab-color. Sheet-add (.sheet-add): --control-h-sm square, --radius-sm, ghost icon button sharing the .tb-btn state model.
- Scrollbars (overlay, custom) — Track: --scrollbar-track (14px), transparent; vscroll top offset = header height, hscroll left offset = row-header width, both inset --scrollbar-track from the shared corner. Thumb: --scrollbar-thumb (8px) centered (3px inset), --radius-pill, min length 28px, color --scrollbar-thumb-color. States — hover/drag: --scrollbar-thumb-hover; opacity/color transition --duration-fast. Rendered only when the axis overflows; z --z-scrollbar. Offsets must reference the same header-height / row-header-width constants the canvas geometry uses.
- Inline cell editor (.inline-edit) — Position absolute over the active cell, box-sizing border-box, 2px --grid-active-cell border, --radius 0 (cell-flush, Excel-style), --surface-canvas background, --text-primary. Font MUST equal the canvas cell font (default --grid-cell-font-size --font-sans, or the cell's own font/size) — NOT --font-mono — so text does not reflow on commit (fixes current mono-vs-system-ui mismatch). Padding 0 --space-2, auto-grows with row height. States — editing: accent border; invalid: --danger border + 0 0 0 2px --danger-soft (replaces hardcoded rgba); focus is implicit (it owns focus while open).
- Autocomplete (.ac-menu / .ac-item) — Menu: elevation-2 (--surface-raised + --shadow-2), --radius-md, padding --space-1, min-width 260 / max-width 360, max-height ~8 rows with overflow-y auto, z --z-autocomplete (above menus), anchored under the caret/inline editor. Item: flex baseline, padding --space-2 --space-3, --radius-sm, gap --space-3. Name (.ac-name): --font-mono, --text-md/--weight-bold, --text-primary. Signature (.ac-sig): --text-sm, --text-secondary, ellipsis-truncated. States — rest transparent; hover --state-hover; active/keyboard-selected: --accent fill with name + sig in --text-on-accent (replaces the hardcoded #fff).
- Dialogs — Two variants. Anchored panel (settings-panel today): elevation-3 (--surface-raised + --shadow-3), --radius-xl, padding --space-8, section gap --space-8, width 220–320, z --z-menu; row (.set-row) space-between, --text-md, gap --space-6; range input accent-color var(--accent); range-val --font-mono/--text-sm/--text-secondary. Modal variant (future): scrim --surface-scrim at --z-scrim + centered dialog at --z-dialog, --radius-xl, --shadow-3, header/body/footer on --space-6 rhythm, footer buttons right-aligned gap --space-3 (primary = --accent fill/--text-on-accent/hover --accent-hover; secondary = --surface-raised + --border-default). Focus-trapped, Esc closes, focus-visible rings on all controls. General button roles used by .filter-apply/.filter-clear/dialogs: primary (accent fill), secondary (--surface-raised + --border-default, hover --border-strong), destructive (--danger); all --control-h-md, --radius-md, padding 0 --space-6, --weight-semibold, disabled = --text-disabled + no hover.
- Inputs (formula bar, cell-ref chip, find fields) — Text inputs: --surface-sunken, 1px --border-default, --radius-md, padding --space-3 --space-4; formula input uses --font-mono/--text-lg (14px), find fields --text-md at --control-h-sm. States — focus: --accent border + --shadow-focus (currently there is NO focus style); invalid: --danger border. Cell-ref chip (.cell-ref): --surface-raised, 1px --border-default, --radius-md, --font-mono/--weight-semibold, min-width 56, centered. Find bar container: elevation-2 (--surface-raised + --shadow-2), --radius-lg, padding --space-3 --space-4, z --z-overlay.

### Adoption steps

1. Author a tokens layer in style.css :root covering the full semantic set above for light, plus matching `@media (prefers-color-scheme: dark)` and `:root[data-theme=light\|dark]` blocks (the manual toggle must win). Keep the legacy 8 tokens as one-release aliases mapping to the new roles (--bg->--surface-canvas, --card->--surface-raised, --border->--border-default, --muted->--text-secondary, --mono->--font-mono) so editor.js keeps rendering unchanged while CSS migrates.
2. Introduce the canvas/grid tokens (--grid-header-bg, --grid-header-active, --grid-active-cell, --grid-frozen-divider, --accent-selection, --grid-cell-font-size, --grid-header-font-size) and update editor.js: the `colors` map reads the new names, `sel` reads --accent-selection instead of `accent+"22"`, and the hardcoded `12px system-ui` / `13px system-ui` strings become the two font-size tokens with --font-sans. This makes chrome and canvas share one source of truth.
3. Sweep editor.css and replace every literal with a token: radii (5/6/7/8/10/12/14 -> --radius-*), gaps/paddings -> --space-*, the three reds (#e5484d, #e53935) -> --danger/--danger-soft, the four shadows -> --shadow-2/--shadow-3, the two #fff on accent (.ac-item.active, .filter-apply) -> --text-on-accent, control heights 34/28 -> --control-h-md/--control-h-sm, z-indexes 6/20/30/40 -> --z-*.
4. Unify the hover model: delete the contradictory `.popmenu button:hover{background:var(--bg);border-color:var(--border)}` and `.filter-item:hover{background:var(--card)}`, and route every menu/filter/tab/icon-button hover through --state-hover (and press through --state-active). Extract a shared `.menu-item` class so popmenu, filter-item, valign/numfmt/border/freeze/sort items share one spec.
5. Add a global focus-visible ring — `:where(button,select,input,label,[tabindex]):focus-visible{ box-shadow: var(--shadow-focus); outline: none }` — plus `#grid:focus-visible{ box-shadow: var(--shadow-focus) }`, and remove ad-hoc `outline:none`/custom outlines. This closes the current gap where controls and inputs show no keyboard focus.
6. Normalize interactive components onto the shared state set (rest/hover/active/focus-visible/selected/disabled): make .tb-btn, .sheet-tab, .sheet-add, and menu items behave identically, and make every toggle (.tb-toggle, active tab, selected menu item) use --accent-soft/--accent uniformly plus a non-color cue (filled state / check glyph).
7. Fix the inline editor: change .inline-edit `font` from --mono to the canvas cell font (--grid-cell-font-size --font-sans, or the cell's own style) so committed text does not shift, and move its `.invalid` state onto --danger + --danger-soft.
8. Standardize control heights and radii app-wide: toolbar 34->--control-h-md (32), find/sheet-add 28->--control-h-sm, formula row ->--control-h-lg; converge all popover/dialog radii on --radius-lg/--radius-xl.
9. Tokenize the scrollbars (sizes and thumb colors) and make their track offsets reference the shared header-height / row-header-width geometry constants used by the canvas, so chrome and grid stay pixel-aligned.
10. Track the work per CLAUDE.md 'no untracked work': add a row in docs/14-EXECUTION-TRACKER.md and a short token-contract note (which tokens the canvas consumes) so editor.js and editor.css cannot drift; optionally ship a token-preview page.
11. Validate and verify: check light + dark contrast (AA 4.5:1 text, 3:1 gridlines/borders/icons/focus ring) for every token pair, then re-verify toolbar, menus, tabs, inline editor, autocomplete, and the settings dialog in-browser in both themes and both accent extremes, per the existing parity-tracker verification discipline. Once green, delete the legacy alias tokens.
