/// The ribbon's tab, group and control assignment — `docs/91` §3, machine-read.
///
/// **Generated from the design note rather than transcribed from it.** §3 is
/// eight tables, and a hand-copy of that is a hundred-odd chances to put Merge
/// & Center in the wrong group. The note is the source; this is its projection.
/// When §3 changes, regenerate — do not hand-edit.
///
/// **One control per verb, and each verb in one place.** Two rules the first
/// generation got wrong, both visible immediately:
///
///  - §3.0 R3 lists an *alias* beside the owning id on some rows —
///    `toolbar.bold (+ format.bold alias)` — meaning one verb reached by two
///    names. Emitting both drew two Bolds, two Italics and two Underlines side
///    by side, at different stroke weights, because one was the toolbar's own
///    artwork and the other the sprite's. A row yields more than one control
///    only when its *name* says so: "Increase / Decrease Font Size" is a pair,
///    "Bold" is not.
///  - A handful of ids appear in two tables. First placement wins, so a command
///    has exactly one home and `listCommands()` cannot double-count it.
///
/// Only rows the note marks `live` are here. A `gap` — Excel has it, this build
/// does not — is deliberately absent rather than drawn disabled: §8 refuses a
/// permanently-dead control, because 150 of them advertise absence in 150
/// places on every screen.
///
/// `size` is the note's L/M/S. `ord` is its authored collapse order, lowest
/// first — §3.0 R6 is explicit that the order is authored and never measured.

export const ASSIGN = [
  { id: "home", label: "Home", groups: [
    { label: "Clipboard", ord: 6, items: [["edit.paste", "large"], "edit.paste-special", "edit.paste-values", ["edit.cut", "med"], ["edit.copy", "med"], ["toolbar.painter", "med"]] },
    { label: "Font", ord: 9, items: [["toolbar.font", "med"], ["toolbar.size", "med"], "toolbar.size-up", "toolbar.size-down", "toolbar.bold", "toolbar.italic", "toolbar.underline", "toolbar.strike", "format.superscript", "format.subscript", "toolbar.border", "toolbar.fillcolor", "toolbar.fontcolor", "format.format-cells"] },
    { label: "Alignment", ord: 8, items: ["format.alignment.top", "format.alignment.justify-vertical", "toolbar.rotate", "toolbar.wrap", "format.alignment.left", "format.alignment.fill-repeat-text", "toolbar.indent-less", "toolbar.indent-more", "toolbar.merge"] },
    { label: "Number", ord: 7, items: [["toolbar.numfmt", "med"], "toolbar.currency", "toolbar.percent", "toolbar.comma", "toolbar.inc-dec", "toolbar.dec-dec"] },
    { label: "Styles", ord: 5, items: [["format.conditional-formatting", "large"], "format.conditional-formatting-rules", ["home.styles.format-as-table", "large"], ["format.cell-styles", "large"]] },
    { label: "Cells", ord: 4, items: [["insert.rows-above", "large"], "insert.cells", "insert.rows-below", "insert.sheet", ["insert.delete-rows", "large"], "insert.delete-cells", "toolbar.delete-sheet", ["home.cells.format", "large"], "format.row-height", "format.column-width", "format.autofit-row", "format.autofit-column", "data.hide-rows", "data.hide-columns", "data.unhide-rows-columns-in-selection", "data.unhide-all-rows-and-columns", "sheet.rename"] },
    { label: "Editing", ord: 99, items: ["formulas.insert-function", ["edit.fill", "med"], "edit.fill.fill-down", "edit.fill.fill-series", ["edit.clear", "med"], "edit.clear.all", ["data.sort-range", "large"], "toolbar.filter", "data.clear-all-filters", ["edit.find-replace", "large"], "edit.replace", "edit.go-to", "edit.select-all", "edit.select-commented"] },
  ] },
  { id: "insert", label: "Insert", groups: [
    { label: "Tables", ord: 11, items: [["insert.pivottable", "large"], "data.pivottable-fields", ["insert.table", "large"], "table.convert-to-range"] },
    { label: "Charts", ord: 99, items: ["insert.chart.column", "insert.chart.bar", "insert.chart.line", "insert.chart.area", "insert.chart.pie", "insert.chart.doughnut", "insert.chart.scatter"] },
    { label: "Links", ord: 9, items: [["insert.hyperlink", "large"], "insert.hyperlink.edit"] },
    { label: "Comments", ord: 8, items: [["insert.note", "large"], "insert.note.show"] },
  ] },
  { id: "pagelayout", label: "Page Layout", groups: [
    { label: "Page Setup", ord: 99, items: [["file.page-break-here", "large"], ["file.page-setup", "large"]] },
    { label: "Sheet Options", ord: 4, items: ["view.gridlines", "view.cell-markings"] },
  ] },
  { id: "formulas", label: "Formulas", groups: [
    { label: "Defined Names", ord: 4, items: [["tools.name-manager", "large"], ["formulas.define-name", "med"], ["name-box-list", "med"]] },
    { label: "Formula Auditing", ord: 5, items: [["format.trace.trace-precedents", "med"], ["view.formulas-instead-of-results", "med"]] },
    { label: "Calculation", ord: 3, items: [["tools.calculation", "large"], "tools.calculation.automatic", ["tools.calculation.calculate-now", "med"]] },
  ] },
  { id: "data", label: "Data", groups: [
    { label: "Queries & Connections", ord: 1, items: [["data.refresh-all-pivots", "large"]] },
    { label: "Sort & Filter", ord: 8, items: ["data.sort-range.a-z", ["data.sort-range.custom-sort", "large"]] },
    { label: "Data Tools", ord: 7, items: [["data.text-to-columns", "med"], ["data.convert-text-to-numbers", "med"], ["data.remove-duplicates", "med"], ["data.data-validation", "med"]] },
    { label: "Outline", ord: 6, items: [["data.group.group-rows", "large"], "data.group.group-columns", ["data.group.ungroup-rows", "large"], "data.group.ungroup-columns", "data.group.expand-all", "data.group.show-level-1"] },
    { label: "Analysis", ord: 5, items: [["data.column-stats", "large"]] },
  ] },
  { id: "review", label: "Review", groups: [
    { label: "Language", ord: 99, items: [["review.language", "med"]] },
    { label: "Protect", ord: 7, items: [["format.protection.protect-this-sheet", "large"], ["format.protection.locked", "large"], ["format.protection.hide-formula", "large"]] },
  ] },
  { id: "view", label: "View", groups: [
    { label: "Sheet View", ord: 3, items: [["data.clear-my-view", "med"]] },
    { label: "Show", ord: 5, items: ["view.zero-values"] },
    { label: "Zoom", ord: 6, items: [["view.zoom", "large"], "view.zoom.50", ["view.zoom.100", "large"]] },
    { label: "Window", ord: 7, items: [["view.freeze", "med"], "view.freeze.up-to-selection"] },
    { label: "Collaboration", ord: 2, items: [["file.version-history", "large"]] },
  ] },
  { id: "help", label: "Help", groups: [
    { label: "Help", ord: 1, items: [["help.keyboard-shortcuts", "large"]] },
  ] },
];

/// Normalise an entry to `{cmd, size}`. A bare string is a small control, which
/// is most of them, so the model stays readable at a glance.
export const item = (e) => (typeof e === "string" ? { cmd: e, size: "small" } : { cmd: e[0], size: e[1] });
