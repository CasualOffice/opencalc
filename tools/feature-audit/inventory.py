#!/usr/bin/env python3
"""Inventory every feature surface and how honestly it is implemented.

Four independent questions per feature, because a feature can pass any one of
them while failing the others — and "it works" usually means only the first:

  UI         is it reachable? (a command, toolbar button, dialog or panel)
  MODEL      does it change the workbook model, rather than editor-local state?
  UNDO       does the change go through `Operation`, so undo can reverse it?
  ROUNDTRIP  does the change survive a save and reopen?
  RENDER     does anything draw it?

Run with --json for the raw rows.
"""
import json, pathlib, re, sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[2]
WASM = (ROOT / "crates/casual-calc-wasm/src/lib.rs").read_text()
JS = (ROOT / "webapp/editor.js").read_text()
HTML = (ROOT / "webapp/editor.html").read_text()
EXPORT = (ROOT / "crates/casual-calc-export/src/lib.rs").read_text()
RENDER = "\n".join(
    p.read_text()
    for d in ("crates/casual-calc-render/src", "crates/casual-calc-layout/src")
    for p in (ROOT / d).rglob("*.rs")
)


# Private helpers that themselves go through `Operation`. A function delegating
# to one of these is undoable even though its own body never mentions the type —
# the first version of this tool read the bodies only and reported every
# toolbar styling command as un-undoable, which is the opposite of the truth.
UNDOABLE_HELPERS = [
    name
    for name, body in re.findall(r"\nfn (\w+)\s*\([^)]*\)[^{]*\{(.*?)\n\}", WASM, re.S)
    if "session.edit(" in body or "EditOperation" in body
]


def wasm_functions():
    """{name: (mutates, undoable)} for every exported session function."""
    out = {}
    for name, body in re.findall(
        r"pub fn (session_\w+|theme_colors|detect_\w+)\s*\([^)]*\)[^{]*\{(.*?)\n\}", WASM, re.S
    ):
        direct = "session.edit(" in body or "EditOperation" in body
        delegated = any(re.search(rf"\b{h}\s*\(", body) for h in UNDOABLE_HELPERS)
        undoable = direct or delegated
        mutates = undoable or "workbook_mut()" in body
        out[name] = (mutates, undoable)
    return out


# Feature areas, each matching one or more wasm function name fragments. Written
# out rather than derived from prefixes because the names do not group cleanly:
# `session_set_fill` and `session_apply_cell_style` are the same feature.
AREAS = {
    "Cell values and formulas": ["set_cell", "clear_range", "clear_contents"],
    "Number formats": ["number_format", "cell_format"],
    "Fonts and text style": ["bold", "italic", "underline", "strike", "font_", "set_font"],
    "Fill and colours": ["set_fill", "font_color", "theme_colors"],
    "Borders": ["border"],
    "Alignment, wrap, indent, rotation": ["align", "wrap", "indent", "rotation", "valign"],
    "Named cell styles": ["cell_style", "named_style"],
    "Rows and columns (size, hide, outline)": ["row_height", "col_width", "hide_", "unhide", "outline", "group"],
    "Insert/delete rows, columns, cells": ["insert_rows", "insert_columns", "delete_rows", "delete_columns", "shift_cells"],
    "Merge": ["merge"],
    "Freeze panes": ["freeze"],
    "Sheets (add, rename, reorder, tab colour)": ["add_sheet", "rename_sheet", "move_sheet", "delete_sheet", "tab_color", "sheet_names"],
    "Sheet visibility": ["sheet_visibility"],
    "Sheet protection": ["sheet_protect"],
    "Clipboard and paste special": ["clip_", "paste"],
    "Fill handle and series": ["fill_mode", "session_fill", "detect_"],
    "Sort": ["sort"],
    "Autofilter": ["filter"],
    "Data validation": ["validation"],
    "Conditional formatting": ["_cf", "cf_"],
    "Comments and threads": ["comment"],
    "Hyperlinks": ["hyperlink"],
    "Tables (ListObjects)": ["table"],
    "Defined names": ["defined_name", "name_"],
    "Undo and redo": ["undo", "redo"],
    "Find and replace": ["find", "replace"],
    "Import and export": ["open_", "save", "import_summary"],
    "Print setup": ["print"],
    "Charts, drawings, images": ["drawing", "chart", "image"],
}

# Which model field or element each area needs the writer to emit. Checked
# against the exporter so "round-trips" is evidence, not assertion.
EXPORT_MARKERS = {
    "Cell values and formulas": "<c r=",
    "Number formats": "numFmtId",
    "Fonts and text style": "<font>",
    "Fill and colours": "patternFill",
    "Borders": "<border",
    "Alignment, wrap, indent, rotation": "<alignment",
    "Named cell styles": "cellStyles",
    "Rows and columns (size, hide, outline)": "<col ",
    "Merge": "mergeCell",
    "Freeze panes": "<pane ",
    "Sheets (add, rename, reorder, tab colour)": "tabColor",
    "Sheet visibility": "state=",
    "Sheet protection": "sheetProtection",
    "Sort": "sortState",
    "Autofilter": "autoFilter",
    "Data validation": "dataValidation",
    "Conditional formatting": "conditionalFormatting",
    "Comments and threads": "commentList",
    "Hyperlinks": "<hyperlink",
    "Tables (ListObjects)": "tableParts",
    "Defined names": "definedName",
    "Print setup": "pageMargins",
    "Charts, drawings, images": "retained_parts",
}

RENDER_MARKERS = {
    "Cell values and formulas": "value",
    "Number formats": "number_format",
    "Fonts and text style": "bold",
    "Fill and colours": "fill_color",
    "Borders": "border",
    "Alignment, wrap, indent, rotation": "align",
    "Merge": "merge",
    "Freeze panes": "frozen",
    "Conditional formatting": "conditional",
    "Comments and threads": "comment",
    "Hyperlinks": "hyperlink",
    "Tables (ListObjects)": "tables",
}


def main():
    fns = wasm_functions()
    rows = []
    for area, fragments in AREAS.items():
        matched = [n for n in fns if any(f in n for f in fragments)]
        mutating = [n for n in matched if fns[n][0]]
        undoable = [n for n in matched if fns[n][1]]
        ui = any(
            frag in JS or frag in HTML
            for frag in [area.split()[0].lower(), *(f for f in fragments)]
        )
        marker = EXPORT_MARKERS.get(area)
        rows.append(
            {
                "area": area,
                "wasm_fns": len(matched),
                "mutating": len(mutating),
                "undoable": len(undoable),
                "no_undo": sorted(set(mutating) - set(undoable)),
                "ui": ui,
                "roundtrip": bool(marker and marker in EXPORT),
                "render": bool(
                    area in RENDER_MARKERS
                    and (RENDER_MARKERS[area] in RENDER or RENDER_MARKERS[area] in JS)
                ),
            }
        )

    if "--json" in sys.argv:
        json.dump(rows, sys.stdout, indent=1)
        return
    print(f"{'AREA':<42}{'FNS':>4}{'MUT':>5}{'UNDO':>6}{'RT':>4}{'RENDER':>8}")
    for r in rows:
        flag = "" if not r["no_undo"] else f"  <- {len(r['no_undo'])} not undoable"
        print(
            f"{r['area']:<42}{r['wasm_fns']:>4}{r['mutating']:>5}{r['undoable']:>6}"
            f"{'yes' if r['roundtrip'] else 'NO':>4}{'yes' if r['render'] else 'NO':>8}{flag}"
        )


if __name__ == "__main__":
    main()
