# 95 — The Excel shell: target and build order

**Status: Accepted by the owner, 2026-10-14.** The approved visual reference is
`docs/prototypes/excel-shell-2026/` (`Main.dc.html` is the desktop shell;
`Laptop`, `Simplified`, `File` and `Phone` are its states). Where this note and
the prototype disagree, the prototype wins; where the prototype is silent,
Excel's own behaviour wins.

## The decision

The editor's shell becomes Excel's shell: same structure, behaviour, keyboard
and dialogs, so an Excel user needs no teaching. Only what Microsoft owns is
replaced — name, logo, the green colour scheme, icon artwork and fonts. The
second rule is lowest cognitive load: anything that makes a person think about
the tool instead of the data is cut.

This overrides three earlier decisions:

1. **Classic ribbon is the default** (docs/91 §4.1 chose Simplified). Simplified
   stays as Excel's toggle.
2. **One shell.** The Sheets chrome (docs/93) and the toolbar mode are retired.
3. **Phones follow Excel mobile**: a bottom command bar with a tab picker that
   expands into a sheet. This settles the docs/93 vs docs/94 conflict.

## What is adopted, what is ours

| Adopted from Excel | Ours |
| --- | --- |
| Title bar with Quick Access Toolbar, Alt+Q search, Comments, Share | Logo, name, OpenCalc green `#0e7a55` accent (owner, 2026-10-14; deliberately not Excel's `#217346`/`#107C41`) |
| Tab set and per-tab group set and order; contextual tabs; collapse order | Icon artwork: Fluent UI System Icons (`@fluentui/svg-icons`, MIT) |
| File page: Home/New, Open, Info, Save As, Print, Share, Export, Options | Templates and help content |
| Every shortcut, two-level KeyTips, Ready/Enter/Edit/Point modes | — |
| Format Cells (six tabs with categories), Paste Special, Insert Function | Visual styling |
| Status bar: mode, Sum/Average/Count, view buttons, zoom | Sync/collaboration state |
| Font metrics | Inter for UI; Carlito (Calibri-metric, OFL) for cells |

## Rules that decide every screen

1. **Icons, not text.** No text glyph stands in for an icon (A▲, .0→, bare $).
   Every icon has a tooltip with name and shortcut. Group captions and button
   labels are a setting, off by default.
2. **Excel's proportions.** Small controls 24px tall with 16px icons; large
   buttons 24px icons in a 72px column; three small rows per column.
3. **One place per command**, plus the right-click menu and search.
4. **Show only what applies now** (contextual tabs; disabled in place, never moved).
5. **One mode, always shown; Escape always returns to Ready on the grid.**
6. **Defaults need no decision** — a new workbook is blank.
7. **Nothing that is for us** — no version strings or debug text.

## Build status (2026-10-14)

Built: the registry and command policy, the ribbon, title bar, search, File
page and phone bar (UX-SHELL-01–08, 13–15, 18). Partial: one keydown router
(09), tabs limited to what the engine can do and no contextual tabs yet (11),
the title-bar Comments button, File-page templates and
Recent (12), the last text glyphs outside the shell (16). Open: the style
lint gate (10) and the task-timing UX gate (17).

## Build order

Rows `UX-SHELL-01..17` in [14](14-EXECUTION-TRACKER.md). Phase 0 (01–05) stops
input loss and is independent of the rebuild. Phase 1 (06–10) is the structural
change: a command registry that the ribbon, context menu, shortcuts and search
all render from, so no chrome borrows another's DOM nodes. Phase 2 (11–17) fills
the content against the prototype.
