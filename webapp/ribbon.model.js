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
/// `split: true` with a `menu` is Excel's split button (AutoSum): the face
/// runs the command, the arrow drops the menu.
///   C(id, width, label)                     a combo (font, size, number format)
///   R([[S…], [S…], …])                      a column of up to three rows
///   X(id, label, title)                     a check box (Sheet Options, Show)
///   T(label)                                a small heading over check boxes
/// `menu` is a list the button drops. An entry is a command id; `"-"`, a
/// separator; `{ label, menu }`, a cascading submenu (Highlight Cells Rules ›);
/// or `{ of: id }`, the entries a registered command lists — the functions of
/// a Function Library category, which are data. A toolbar control that
/// already owns a popup (borders, colours, merge) is marked `popup: true` and
/// opens that popup under the ribbon button instead.
/// `fold` on a group is its collapse rank — lowest folds first (Excel folds
/// Styles, Cells, Editing before it touches Font or Clipboard).

const L = (id, icon, label, o = {}) => ({ kind: "large", id, icon, label, ...o });
const S = (id, icon, label, o = {}) => ({ kind: "small", id, icon, label, ...o });
const C = (id, width, label) => ({ kind: "combo", id, width, label });
const R = (rows) => ({ kind: "rows", rows });
const X = (id, label, title) => ({ kind: "check", id, label, title });
const T = (label) => ({ kind: "heading", label });
const SUB = (label, menu) => ({ label, menu });

/// Home ▸ Conditional Formatting, in Excel's order (XL-UX-037). Icon Sets ›
/// and A Date Occurring… are left out until the engine draws icon sets and
/// holds a time-period rule (XL-FEAT-151).
const CF_MENU = [
  SUB("Highlight Cells Rules", [
    "format.cf.greater-than", "format.cf.less-than", "format.cf.between", "format.cf.equal-to",
    "format.cf.text-contains", "format.cf.duplicate-values", "-", "format.cf.more-rules",
  ]),
  SUB("Top/Bottom Rules", [
    "format.cf.top-10-items", "format.cf.top-10-percent", "format.cf.bottom-10-items", "format.cf.bottom-10-percent",
    "format.cf.above-average", "format.cf.below-average", "-", "format.cf.more-rules",
  ]),
  SUB("Data Bars", [
    "format.cf.data-bar.blue", "format.cf.data-bar.green", "format.cf.data-bar.red",
    "format.cf.data-bar.orange", "format.cf.data-bar.light-blue", "format.cf.data-bar.purple", "-", "format.cf.more-rules",
  ]),
  SUB("Color Scales", [
    "format.cf.color-scale.green-yellow-red", "format.cf.color-scale.red-yellow-green",
    "format.cf.color-scale.green-white-red", "format.cf.color-scale.red-white-green",
    "format.cf.color-scale.blue-white-red", "format.cf.color-scale.red-white-blue",
    "format.cf.color-scale.white-red", "format.cf.color-scale.red-white",
    "format.cf.color-scale.green-white", "format.cf.color-scale.white-green",
    "format.cf.color-scale.green-yellow", "format.cf.color-scale.yellow-green", "-", "format.cf.more-rules",
  ]),
  "-",
  "format.cf.new-rule",
  SUB("Clear Rules", ["format.cf.clear-selected", "format.cf.clear-sheet"]),
  "format.cf.manage-rules",
];

/// AutoSum ›, Excel's list (Home ▸ Editing and Formulas ▸ Function Library).
const AUTOSUM = ["formulas.autosum.sum", "formulas.autosum.average", "formulas.autosum.count-numbers",
  "formulas.autosum.max", "formulas.autosum.min", "-", "formulas.autosum.more-functions"];

/// A Function Library category: the functions it lists, by name (XL-UX-041).
const FN = (key, icon, label) => L(`formulas.library.${key}`, icon, label, { menu: [{ of: `formulas.library.${key}` }] });

const PASTE =["edit.paste", "edit.paste-values", "edit.paste-formulas", "edit.paste-formats", "edit.paste-transpose", "edit.paste-special"];

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
      L("format.conditional-formatting", "data_bar_horizontal", "Conditional Formatting", { menu: CF_MENU }),
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
        [S("formulas.autosum", "autosum", "AutoSum (Alt+=)", { menu: AUTOSUM, split: true })],
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
    // The thread panel `insert.note` opens is Excel's Comment — threaded, with
    // replies and Resolve — so it carries that name (XL-UX-084). Notes, the
    // separate sticky note, are XL-UX-039.
    { label: "Comments", fold: 2, icon: "comment_add", items: [L("insert.note", "comment_add", "Comment")] },
  ] },
  // Excel's Page Layout tab (XL-UX-040). Themes, Background and Arrange are
  // left out: there are no workbook themes, sheet backgrounds or drawing
  // objects to arrange. Page Setup, Scale to Fit and Sheet Options open the
  // Page Setup panel from their launchers, as Excel opens its dialog.
  { id: "pagelayout", label: "Page Layout", key: "P", groups: [
    { label: "Page Setup", fold: 4, launcher: "file.page-setup", icon: "document_one_page", items: [
      L("page.margins", "document_margins", "Margins", { menu: ["page.margins.normal", "page.margins.wide", "page.margins.narrow", "-", "page.margins.custom"] }),
      L("page.orientation", "document_landscape", "Orientation", { menu: ["page.orientation.portrait", "page.orientation.landscape"] }),
      L("page.size", "document_one_page", "Size", { menu: ["page.size.letter", "page.size.legal", "page.size.a3", "page.size.a4", "page.size.a5", "-", "page.size.more"] }),
      L("page.print-area", "document_print", "Print Area", { menu: ["page.print-area.set", "page.print-area.clear"] }),
      L("page.breaks", "document_page_break", "Breaks", { menu: ["page.breaks.insert", "page.breaks.remove", "page.breaks.reset"] }),
      L("page.print-titles", "document_header", "Print Titles"),
    ] },
    { label: "Scale to Fit", fold: 3, launcher: "file.page-setup", icon: "page_fit", items: [
      R([[T("Width:"), C("page.fit.width", 96, "Width")], [T("Height:"), C("page.fit.height", 96, "Height")], [T("Scale:"), C("page.fit.scale", 96, "Scale")]]),
    ] },
    { label: "Sheet Options", fold: 2, launcher: "file.page-setup", icon: "grid", items: [
      R([[T("Gridlines")], [X("sheet.gridlines.view", "View", "View Gridlines")], [X("sheet.gridlines.print", "Print", "Print Gridlines")]]),
      R([[T("Headings")], [X("sheet.headings.view", "View", "View Headings")], [X("sheet.headings.print", "Print", "Print Headings")]]),
    ] },
  ] },
  { id: "formulas", label: "Formulas", key: "M", groups: [
    // Excel's Function Library (XL-UX-041): each category drops the functions
    // in it, and choosing one opens Function Arguments. More Functions › has
    // no Cube: the engine implements none of the cube functions.
    { label: "Function Library", fold: 5, icon: "math_formula", items: [
      L("formulas.insert-function", "math_formula", "Insert Function (Shift+F3)"),
      L("formulas.autosum", "autosum", "AutoSum (Alt+=)", { menu: AUTOSUM, split: true }),
      FN("recently-used", "clock", "Recently Used"),
      FN("financial", "money", "Financial"),
      FN("logical", "checkbox_checked", "Logical"),
      FN("text", "text_t", "Text"),
      FN("date-time", "calendar_clock", "Date & Time"),
      FN("lookup-reference", "table_search", "Lookup & Reference"),
      FN("math-trig", "math_symbols", "Math & Trig"),
      L("formulas.more-functions", "more_circle", "More Functions", { menu: [
        SUB("Statistical", [{ of: "formulas.library.statistical" }]),
        SUB("Engineering", [{ of: "formulas.library.engineering" }]),
        SUB("Information", [{ of: "formulas.library.information" }]),
        SUB("Compatibility", [{ of: "formulas.library.compatibility" }]),
        SUB("Web", [{ of: "formulas.library.web" }]),
      ] }),
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
    // Excel's Forecast group. What-If Analysis lists only what is built:
    // Scenario Manager and Data Table are not, and are left out rather than
    // drawn dead (XL-FEAT-032).
    { label: "Forecast", fold: 1, icon: "data_line", items: [
      L("data.goal-seek", "data_line", "What-If Analysis", { menu: ["data.goal-seek"] }),
    ] },
    { label: "Outline", fold: 4, icon: "table_cells_merge", items: [
      L("data.group", "table_cells_merge", "Group", { menu: ["data.group.group-rows", "data.group.group-columns"] }),
      L("data.ungroup", "table_delete_row", "Ungroup", { menu: ["data.group.ungroup-rows", "data.group.ungroup-columns"] }),
      R([[S("data.group.expand-all", "add", "Show Detail")], [S("data.group.collapse-all", "subtract", "Hide Detail")]]),
    ] },
    { label: "Analysis", fold: 3, icon: "data_bar_vertical", items: [L("data.column-stats", "data_bar_vertical", "Column Statistics")] },
  ] },
  { id: "review", label: "Review", key: "R", groups: [
    // Excel's Comments group (XL-UX-042). The thread panel is the comment;
    // Show Comments (the all-threads pane, XL-FEAT-046) is left out until it
    // exists, and so is the Notes group until a note is its own thing
    // (XL-UX-039) — "New Note" here opened the comment thread.
    { label: "Comments", fold: 3, icon: "comment_add", items: [
      L("insert.note", "comment_add", "New Comment"),
      L("review.delete-comment", "comment_dismiss", "Delete"),
      L("review.previous-comment", "comment_arrow_left", "Previous"),
      L("review.next-comment", "comment_arrow_right", "Next"),
    ] },
    { label: "Protect", fold: 4, icon: "lock_closed", items: [
      L("format.protection.protect-this-sheet", "lock_closed", "Protect Sheet"),
      R([[S("format.protection.locked", "lock_closed", "Lock Cell")], [S("format.protection.hide-formula", "eye", "Hide Formula")]]),
    ] },
  ] },
  { id: "view", label: "View", key: "W", groups: [
    { label: "Sheet View", fold: 2, icon: "eye", items: [L("data.clear-my-view", "eye", "Exit Sheet View")] },
    // Excel's Show group, as its check boxes (XL-UX-043). Ruler belongs to Page
    // Layout view, which is not built. Zero Values is an Excel Option, not a
    // ribbon control, and stays reachable through command search.
    { label: "Show", fold: 3, icon: "grid", items: [
      R([[X("sheet.gridlines.view", "Gridlines", "Gridlines")], [X("view.show.formula-bar", "Formula Bar", "Formula Bar")], [X("sheet.headings.view", "Headings", "Headings")]]),
    ] },
    { label: "Zoom", fold: 5, icon: "zoom_in", items: [
      L("view.zoom.dialog", "zoom_in", "Zoom"),
      L("view.zoom.100", "search", "100%"),
      L("view.zoom.selection", "zoom_fit", "Zoom to Selection"),
    ] },
    // Window: Freeze Panes only. New Window, Arrange All, Split, Hide/Unhide
    // (of the workbook window) and View Side by Side need windows this
    // single-document page does not have.
    { label: "Window", fold: 6, icon: "table_freeze_column_and_row", items: [
      L("view.freeze", "table_freeze_column_and_row", "Freeze Panes", { menu: ["view.freeze.panes", "view.freeze.top-row-excel", "view.freeze.first-column-excel"] }),
    ] },
    { label: "Collaboration", fold: 4, icon: "history", items: [L("file.version-history", "history", "Version History")] },
    // ONLYOFFICE keeps Macros on its View tab (docs/99 §3), and so do we.
    { label: "Macros", fold: 1, icon: "code", items: [L("view.macros", "code", "Macros (Alt+F8)")] },
  ] },
  // Excel's Developer tab. Only what is built: Record Macro (MACRO-03) and
  // Convert VBA arrive with their rows, not as dead buttons now.
  { id: "developer", label: "Developer", key: "L", groups: [
    // Excel's order: Visual Basic, then Macros. Visual Basic (MACRO-05) is the
    // read-only VBA view, there only when the workbook carries a VBA project.
    { label: "Code", fold: 1, icon: "code", items: [L("developer.visual-basic", "document", "Visual Basic (Alt+F11)"), L("developer.macros", "code", "Macros (Alt+F8)")] },
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
export const OPTIONS_COMMANDS = ["view.theme.auto", "view.theme.light", "view.theme.dark", "file.trust-center", "tools.settings"];

/// Every command id a menu names, through its submenus: `visit(id)` for each
/// command entry and each `{ of }` list.
export function menuIds(menu, visit) {
  for (const m of menu) {
    if (m === "-") continue;
    if (typeof m === "string") visit(m);
    else if (m.of) visit(m.of);
    else if (m.menu) menuIds(m.menu, visit);
  }
}

/// Every command id the model names, for the registry-completeness gate.
export function modelCommandIds() {
  const ids = new Set();
  const walk = (it) => {
    if (it.kind === "rows") { for (const row of it.rows) row.forEach(walk); return; }
    if (it.kind === "heading") return;
    ids.add(it.id);
    menuIds(it.menu || [], (m) => ids.add(m));
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
  for (const id of ["cells.insert", "cells.delete", "cells.format", "data.ungroup",
    "page.margins", "page.orientation", "page.size", "page.print-area", "page.breaks", "formulas.more-functions"]) ids.delete(id);
  return [...ids];
}
