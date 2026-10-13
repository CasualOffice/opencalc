/// The ribbon, as data — UX-SHELL-07.
///
/// Mirrors the owner-approved prototype (`docs/prototypes/excel-shell-2026/
/// Main.dc.html`, docs/95): Excel's tabs, Excel's groups in Excel's order, and
/// within a group Excel's arrangement of large buttons and three-row stacks.
///
/// Every id names a command the registry (`commands.js`) can run. A verb Excel
/// has and this build does not — Pictures, Shapes, Text Box — is left out
/// rather than drawn dead: a disabled button that never enables advertises an
/// absence on every screen (docs/91 §8).
///
/// Shapes:
///   L(id, icon, label, {menu?, launcher?})  a large button: 24px icon, 72px column
///   S(id, icon, label, {menu?})             a small button: 16px icon, 24px row
///   C(id, width, label)                     a combo (font, size, number format)
///   R([[S…], [S…], …])                      a column of up to three rows
/// `menu` is a list of command ids the button drops. A toolbar control that
/// already owns a popup (borders, colours, merge) is marked `popup: true` and
/// opens that popup under the ribbon button instead.
/// `fold` on a group is its collapse rank — lowest folds first (Excel folds
/// Styles, Cells, Editing before it touches Font or Clipboard).

const L = (id, icon, label, o = {}) => ({ kind: "large", id, icon, label, ...o });
const S = (id, icon, label, o = {}) => ({ kind: "small", id, icon, label, ...o });
const C = (id, width, label) => ({ kind: "combo", id, width, label });
const R = (rows) => ({ kind: "rows", rows });

const PASTE = ["edit.paste", "edit.paste-values", "edit.paste-formulas", "edit.paste-formats", "edit.paste-transpose", "edit.paste-special"];

export const TABS = [
  { id: "home", label: "Home", key: "H", groups: [
    { label: "Clipboard", fold: 6, launcher: "edit.paste-special", icon: "clipboard_paste", items: [
      L("edit.paste", "clipboard_paste", "Paste (Ctrl+V)", { menu: PASTE }),
      R([[S("edit.cut", "cut", "Cut (Ctrl+X)")], [S("edit.copy", "copy", "Copy (Ctrl+C)")], [S("toolbar.painter", "paint_brush", "Format Painter")]]),
    ] },
    { label: "Font", fold: 9, launcher: "format.format-cells", icon: "text_color", items: [
      R([
        [C("toolbar.font", 132, "Font"), C("toolbar.size", 52, "Font Size"), S("toolbar.size-up", "font_increase", "Increase Font Size (Ctrl+Shift+>)"), S("toolbar.size-down", "font_decrease", "Decrease Font Size (Ctrl+Shift+<)")],
        [S("toolbar.bold", "text_bold", "Bold (Ctrl+B)"), S("toolbar.italic", "text_italic", "Italic (Ctrl+I)"), S("toolbar.underline", "text_underline", "Underline (Ctrl+U)"), S("toolbar.strike", "text_strikethrough", "Strikethrough (Ctrl+5)"),
         S("toolbar.border", "border_bottom", "Borders", { popup: true }), S("toolbar.fillcolor", "paint_bucket", "Fill Color", { popup: true }), S("toolbar.fontcolor", "text_color", "Font Color", { popup: true })],
      ]),
    ] },
    { label: "Alignment", fold: 8, launcher: "format.format-cells", icon: "text_align_left", items: [
      R([
        [S("format.alignment.top", "align_top", "Top Align"), S("format.alignment.middle", "align_center_vertical", "Middle Align"), S("format.alignment.bottom", "align_bottom", "Bottom Align"),
         S("toolbar.rotate", "text_direction_rotate_270_right", "Orientation", { popup: true }), S("format.text-overflow.wrap", "text_wrap", "Wrap Text")],
        [S("format.alignment.left", "text_align_left", "Align Left"), S("format.alignment.center", "text_align_center", "Center"), S("format.alignment.right", "text_align_right", "Align Right"),
         S("toolbar.indent-less", "text_indent_decrease", "Decrease Indent"), S("toolbar.indent-more", "text_indent_increase", "Increase Indent"), S("toolbar.merge", "table_cells_merge", "Merge & Center", { popup: true })],
      ]),
    ] },
    { label: "Number", fold: 7, launcher: "format.format-cells", icon: "percent", items: [
      R([
        [C("toolbar.numfmt", 150, "Number Format")],
        [S("toolbar.currency", "currency_dollar_euro", "Currency Format"), S("toolbar.percent", "percent", "Percent Style (Ctrl+Shift+%)"), S("toolbar.comma", "comma", "Comma Style"),
         S("toolbar.inc-dec", "decimal_arrow_right", "Increase Decimal"), S("toolbar.dec-dec", "decimal_arrow_left", "Decrease Decimal")],
      ]),
    ] },
    { label: "Styles", fold: 1, icon: "style_guide", items: [
      L("format.conditional-formatting", "data_bar_horizontal", "Conditional Formatting", { menu: ["format.conditional-formatting", "format.conditional-formatting-rules"] }),
      L("insert.table", "table", "Format as Table (Ctrl+T)"),
      L("format.cell-styles", "style_guide", "Cell Styles"),
    ] },
    { label: "Cells", fold: 2, icon: "table_insert_row", items: [
      L("cells.insert", "table_insert_row", "Insert", { menu: ["insert.rows-above", "insert.rows-below", "insert.columns-left", "insert.columns-right", "insert.cells-down", "insert.cells-right", "insert.sheet"] }),
      L("cells.delete", "table_delete_row", "Delete", { menu: ["insert.delete-rows", "insert.delete-columns", "insert.delete-cells-up", "insert.delete-cells-left", "toolbar.delete-sheet"] }),
      L("cells.format", "table_settings", "Format", { menu: ["format.row-height", "format.autofit-row", "format.column-width", "format.autofit-column", "data.hide-rows", "data.hide-columns", "data.unhide-rows-columns-in-selection", "sheet.rename", "format.protection.protect-this-sheet", "format.format-cells"] }),
    ] },
    { label: "Editing", fold: 3, icon: "search", items: [
      R([
        [S("formulas.autosum", "autosum", "AutoSum (Alt+=)", { menu: ["formulas.autosum", "formulas.insert-function"] })],
        [S("edit.fill", "arrow_down", "Fill", { menu: ["edit.fill.fill-down", "edit.fill.fill-right", "edit.fill.fill-series", "edit.fill.growth-series", "edit.fill.copy-cells", "edit.fill.formatting-only", "edit.fill.without-formatting"] })],
        [S("edit.clear", "eraser", "Clear", { menu: ["edit.clear.all", "edit.clear.formatting", "edit.clear.values"] })],
      ]),
      L("data.sort-range", "arrow_sort_down_lines", "Sort & Filter", { menu: ["data.sort-range.a-z", "data.sort-range.z-a", "data.sort-range.custom-sort", "data.filter", "data.clear-all-filters"] }),
      L("edit.find-replace", "search", "Find & Select", { menu: ["edit.find-replace", "edit.replace", "edit.go-to", "edit.go-to-special", "edit.select-all"] }),
    ] },
  ] },
  { id: "insert", label: "Insert", key: "N", groups: [
    { label: "Tables", fold: 4, icon: "table", items: [
      L("insert.pivottable", "table_lightning", "PivotTable"),
      L("insert.table", "table", "Table (Ctrl+T)"),
    ] },
    { label: "Charts", fold: 5, icon: "data_bar_vertical", items: [
      R([
        [S("insert.chart.column", "data_bar_vertical", "Column or Bar Chart", { menu: ["insert.chart.column", "insert.chart.bar"] }), S("insert.chart.line", "data_line", "Line or Area Chart", { menu: ["insert.chart.line", "insert.chart.area"] })],
        [S("insert.chart.pie", "data_pie", "Pie or Doughnut Chart", { menu: ["insert.chart.pie", "insert.chart.doughnut"] }), S("insert.chart.scatter", "data_scatter", "Scatter Chart")],
      ]),
    ] },
    { label: "Links", fold: 3, icon: "link", items: [L("insert.hyperlink", "link", "Link (Ctrl+K)")] },
    { label: "Comments", fold: 2, icon: "comment_add", items: [L("insert.note", "comment_add", "Note")] },
  ] },
  { id: "pagelayout", label: "Page Layout", key: "P", groups: [
    { label: "Page Setup", fold: 4, icon: "document_one_page", items: [
      L("file.page-setup", "document_one_page", "Page Setup"),
      L("file.page-break-here", "document_page_break", "Insert Page Break"),
      L("file.print", "print", "Print (Ctrl+P)"),
    ] },
    { label: "Sheet Options", fold: 3, icon: "grid", items: [
      R([[S("view.gridlines", "grid", "Gridlines")], [S("view.cell-markings", "table", "Headings")]]),
    ] },
  ] },
  { id: "formulas", label: "Formulas", key: "M", groups: [
    { label: "Function Library", fold: 5, icon: "math_formula", items: [
      L("formulas.insert-function", "math_formula", "Insert Function (Shift+F3)"),
      L("formulas.autosum", "autosum", "AutoSum (Alt+=)"),
    ] },
    { label: "Defined Names", fold: 4, icon: "table_settings", items: [
      L("tools.name-manager", "table_settings", "Name Manager (Ctrl+F3)"),
      R([[S("formulas.define-name", "add", "Define Name")]]),
    ] },
    { label: "Formula Auditing", fold: 3, icon: "eye", items: [
      R([
        [S("format.trace.trace-precedents", "arrow_import", "Trace Precedents")],
        [S("format.trace.trace-dependents", "arrow_export", "Trace Dependents")],
        [S("format.trace.clear-trace-arrows", "eraser", "Remove Arrows"), S("view.formulas-instead-of-results", "eye", "Show Formulas (Ctrl+`)")],
      ]),
    ] },
    { label: "Calculation", fold: 2, icon: "calculator", items: [
      L("tools.calculation", "calculator", "Calculation Options", { menu: ["tools.calculation.automatic", "tools.calculation.manual"] }),
      R([[S("tools.calculation.calculate-now", "arrow_clockwise", "Calculate Now (F9)")]]),
    ] },
  ] },
  { id: "data", label: "Data", key: "A", groups: [
    { label: "Queries & Connections", fold: 2, icon: "arrow_clockwise", items: [L("data.refresh-all-pivots", "arrow_clockwise", "Refresh All")] },
    { label: "Sort & Filter", fold: 6, icon: "filter", items: [
      R([[S("data.sort-range.a-z", "text_sort_ascending", "Sort A to Z")], [S("data.sort-range.z-a", "text_sort_descending", "Sort Z to A")]]),
      L("data.sort-range.custom-sort", "arrow_sort_down_lines", "Sort"),
      L("data.filter", "filter", "Filter (Ctrl+Shift+L)"),
      R([[S("data.clear-all-filters", "filter_dismiss", "Clear")]]),
    ] },
    { label: "Data Tools", fold: 5, icon: "table_insert_column", items: [
      R([
        [S("data.text-to-columns", "table_insert_column", "Text to Columns")],
        [S("data.remove-duplicates", "table_delete_row", "Remove Duplicates")],
        [S("data.data-validation", "checkmark", "Data Validation"), S("data.convert-text-to-numbers", "number_symbol", "Convert Text to Numbers")],
      ]),
    ] },
    { label: "Outline", fold: 4, icon: "table_cells_merge", items: [
      L("data.group", "table_cells_merge", "Group", { menu: ["data.group.group-rows", "data.group.group-columns"] }),
      L("data.ungroup", "table_delete_row", "Ungroup", { menu: ["data.group.ungroup-rows", "data.group.ungroup-columns"] }),
      R([[S("data.group.expand-all", "add", "Show Detail")], [S("data.group.collapse-all", "subtract", "Hide Detail")]]),
    ] },
    { label: "Analysis", fold: 3, icon: "data_bar_vertical", items: [L("data.column-stats", "data_bar_vertical", "Column Statistics")] },
  ] },
  { id: "review", label: "Review", key: "R", groups: [
    { label: "Notes", fold: 3, icon: "comment_add", items: [L("insert.note", "comment_add", "New Note")] },
    { label: "Protect", fold: 4, icon: "lock_closed", items: [
      L("format.protection.protect-this-sheet", "lock_closed", "Protect Sheet"),
      R([[S("format.protection.locked", "lock_closed", "Lock Cell")], [S("format.protection.hide-formula", "eye", "Hide Formula")]]),
    ] },
  ] },
  { id: "view", label: "View", key: "W", groups: [
    { label: "Sheet View", fold: 2, icon: "eye", items: [L("data.clear-my-view", "eye", "Exit Sheet View")] },
    { label: "Show", fold: 3, icon: "grid", items: [
      R([[S("view.gridlines", "grid", "Gridlines")], [S("view.cell-markings", "table", "Headings")], [S("view.zero-values", "number_symbol", "Zero Values")]]),
    ] },
    { label: "Zoom", fold: 5, icon: "zoom_in", items: [
      L("view.zoom", "zoom_in", "Zoom", { menu: ["view.zoom.50", "view.zoom.75", "view.zoom.100", "view.zoom.150", "view.zoom.200"] }),
      L("view.zoom.100", "search", "100%"),
    ] },
    { label: "Window", fold: 6, icon: "table_freeze_column_and_row", items: [
      L("view.freeze", "table_freeze_column_and_row", "Freeze Panes", { menu: ["view.freeze.up-to-selection", "view.freeze.top-row", "view.freeze.first-column", "view.freeze.unfreeze"] }),
    ] },
    { label: "Collaboration", fold: 4, icon: "history", items: [L("file.version-history", "history", "Version History")] },
  ] },
  { id: "help", label: "Help", key: "Y", groups: [
    { label: "Help", fold: 1, icon: "question_circle", items: [
      L("help.keyboard-shortcuts", "keyboard", "Keyboard Shortcuts"),
      L("help.about-opencalc", "info", "About"),
    ] },
  ] },
];

/// The File page, in Excel's order. Each page is a list of sections; an item
/// names a command, its icon, and one line saying what it does — Excel's File
/// page describes its choices rather than listing bare links. `new` draws
/// template tiles and `options` draws grouped settings instead.
const I = (id, icon, label, desc) => ({ id, icon, label, desc });
export const FILE_RAIL = [
  { id: "new", label: "New", icon: "document_add" },
  { id: "open", label: "Open", icon: "folder_open", sections: [
    { items: [I("file.open", "folder_open", "Browse", "Open an Excel, OpenDocument or CSV file from this device")] },
  ] },
  { id: "info", label: "Info", icon: "info", sections: [
    { items: [
      I("file.properties", "info", "Properties", "Title, subject, author and keywords saved with the workbook"),
      I("file.version-history", "history", "Version History", "See earlier versions of this workbook and restore one"),
    ] },
  ] },
  { id: "save", label: "Save a Copy", icon: "save", sections: [
    { items: [I("file.save", "save", "Save", "Save to the file this workbook was opened from")] },
    { title: "Save a copy as", items: [
      I("file.download.excel-xlsx", "table", "Excel Workbook", ".xlsx — opens in Excel and most spreadsheet apps"),
      I("file.download.excel-macro-enabled-xlsm", "table", "Excel Macro-Enabled Workbook", ".xlsm — keeps the workbook's macros"),
      I("file.download.opendocument-ods", "document", "OpenDocument Spreadsheet", ".ods — for LibreOffice and other ODF apps"),
      I("file.download.csv-csv", "document", "CSV (Comma delimited)", ".csv — values of the active sheet only"),
      I("file.download.tab-separated-tsv", "document", "Text (Tab delimited)", ".tsv — values of the active sheet only"),
      I("file.download.pipe-separated-psv", "document", "Text (Pipe delimited)", ".psv — values of the active sheet only"),
      I("file.download.same-format-as-opened", "arrow_download", "Same format as opened", "Keep the format the file came in"),
    ] },
  ] },
  { id: "print", label: "Print", icon: "print", sections: [
    { items: [
      I("file.print", "print", "Print", "Print the active sheet (Ctrl+P)"),
      I("file.export-as-pdf", "document_pdf", "Export as PDF", "Save the sheet as a PDF document"),
      I("file.page-setup", "document_one_page", "Page Setup", "Margins, orientation, scaling and print titles"),
    ] },
  ] },
  { id: "share", label: "Share", icon: "share", sections: [
    { items: [I("file.share", "people", "Share with people", "Invite others to edit this workbook with you")] },
  ] },
  { id: "options", label: "Options", icon: "settings" },
];

/// Commands the Options page uses (for the completeness gate).
export const OPTIONS_COMMANDS = ["view.theme.auto", "view.theme.light", "view.theme.dark", "tools.settings"];

/// Every command id the model names, for the registry-completeness gate.
export function modelCommandIds() {
  const ids = new Set();
  const walk = (it) => {
    if (it.kind === "rows") { for (const row of it.rows) row.forEach(walk); return; }
    ids.add(it.id);
    for (const m of it.menu || []) ids.add(m);
  };
  for (const tab of TABS) for (const g of tab.groups) {
    g.items.forEach(walk);
    if (g.launcher) ids.add(g.launcher);
  }
  for (const r of FILE_RAIL) for (const sec of r.sections || []) for (const it of sec.items) ids.add(it.id);
  for (const c of OPTIONS_COMMANDS) ids.add(c);
  ids.add("file.new");
  // Group-level wrappers that are not commands themselves: the button only
  // drops its menu.
  for (const id of ["cells.insert", "cells.delete", "cells.format", "data.ungroup"]) ids.delete(id);
  return [...ids];
}
