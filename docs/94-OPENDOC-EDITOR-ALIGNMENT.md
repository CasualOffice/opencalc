# 94 — OpenDoc editor alignment

## Outcome
OpenCalc presents the same editor family as OpenDoc: a document title above
navigation, a quiet blue ribbon, consistent typography, floating surfaces and
status controls. The user selected editor-first scope on 2026-10-13.

## Research
Reference: sibling OpenDoc `../opendoc/webapp/src/style.css`, `../opendoc/webapp/src/fonts.css`,
`../opendoc/webapp/editor.html`
and chrome wiring, inspected 2026-10-13. Existing OpenCalc designs 91 and 93
supply command ownership, keyboard navigation and chrome switching.

## Design — Final
Use the existing ribbon as the browser default, respecting stored preferences
and URL overrides. Align its title/navigation block to OpenDoc's 58px header,
74px command band, Inter chrome and Studio blue palette. Reuse the licensed
Inter asset already selected by OpenDoc; no remote font requests. Retain
Classic/Simplified and the toolbar/Sheets alternatives. Native and hidden-header
hosts keep a standalone tab strip. Match menus, panels, dialogs and footer
through existing semantic tokens, preserving dark theme and host overrides.
The formula bar, name box, sheet tabs and canvas remain spreadsheet surfaces.
Add a keyboard skip link to the grid. Keep undo/redo visible beside the ribbon
navigation across all tabs; use proxies of the existing transaction commands.
On phones, folded groups can swipe horizontally and the engine status gets
a full footer line, so progress and errors remain readable. All commands retain their existing owners.

### Model and layers
Browser presentation only. No changes to workbook data, engine layout,
transactions, SDK API, formats, collaboration or dependency graph.

### Failure modes
Positioned navigation must not overlap title actions, escape the viewport or
remain after a chrome switch. Header-hidden and native modes need visible tabs.
Fonts must load locally and fall back when unavailable. Dark palette remains
explicit. All menu and formatting actions must still reach the existing engine.

## Alternatives
A new editor implementation would duplicate command ownership. A stylesheet-only
reskin would leave a different navigation hierarchy. Reuse the existing ribbon
and its command routing instead.

## Acceptance gates
Real WASM browser boot, formula calculation and editing; ribbon tab and File
navigation; preference/URL precedence; keyboard skip link; layout at 390, 768
and 1440 pixels; light/dark style and local Inter loading. Existing chrome and
smoke suites plus `editor.opendoc-shell.spec.mjs`. Documentation and theme gates.

## ADRs
No engine or public contract changes. This adopts OpenDoc's existing licensed
chrome family without adding a package or font service.

## Tracker
UX-OD-01. Public website redesign is outside the selected editor-first scope.
