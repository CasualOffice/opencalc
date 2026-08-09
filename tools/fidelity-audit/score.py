#!/usr/bin/env python3
"""The fidelity score: how much of the SpreadsheetML semantic core survives.

Prints a per-part and overall percentage so progress is measured rather than
asserted. `--list` names what is still unhandled, in the order worth fixing.

Some constructs are deliberately out of scope and are named in OUT_OF_SCOPE
rather than quietly dropped from the denominator: a score is only honest if what
it excludes is visible. Nothing is excluded for being *hard* — only for being
legacy, application-specific, or outside a spreadsheet engine's job.
"""
import argparse, pathlib, re, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from schema import Schema

ROOTS = {
    "worksheet": "CT_Worksheet", "workbook": "CT_Workbook",
    "styleSheet": "CT_Stylesheet", "sst": "CT_Sst",
    "table": "CT_Table", "comments": "CT_Comments",
}

# Excluded from the denominator, with the reason. Each is a construct Excel
# itself treats as legacy or host-specific, not something we are ducking.
OUT_OF_SCOPE = {
    # Smart tags: removed from Excel in 2010.
    "cellSmartTag", "cellSmartTagPr", "cellSmartTags", "smartTags", "smartTagPr",
    "smartTagType", "smartTagTypes",
    # Excel Web Components / SharePoint publishing: discontinued.
    "webPublishItem", "webPublishItems", "webPublishObject", "webPublishObjects",
    "webPublishing",
    # Embedded binaries and host UI, not spreadsheet semantics.
    "oleObject", "oleObjects", "oleSize", "control", "controlPr", "controls",
    "objectPr", "picture", "drawing", "drawingHF", "anchor", "from", "to",
    # Legacy analysis features with no editor surface.
    "scenario", "scenarios", "inputCells", "dataConsolidate", "dataRef", "dataRefs",
    "cellWatch", "cellWatches", "customPr", "customProperties", "securityDescriptor",
    "customSheetView", "customSheetViews", "customWorkbookView", "customWorkbookViews",
    "fileRecoveryPr", "functionGroup", "functionGroups", "pivotArea", "pivotSelection",
    "reference", "references", "x", "pivotCache", "pivotCaches",
    # East Asian phonetic guides: a distinct typographic feature, tracked apart.
    "phoneticPr", "rPh",
    # Extension points, not constructs.
    "ext", "extLst",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="name what is unhandled")
    args = ap.parse_args()

    S = Schema()
    root = pathlib.Path(__file__).resolve().parents[2]
    # Both crates: `<sheet>` and `<sheets>` are parsed in casual-calc-ooxml's
    # package discovery, not in the importer, and scanning only the importer
    # scored them as unhandled when they are the first thing read.
    imp = set()
    for crate in ("casual-calc-import", "casual-calc-ooxml"):
        for p in (root / f"crates/{crate}/src").glob("*.rs"):
            imp |= set(re.findall(r'b"([\w:.-]+)"', p.read_text()))

    def kids(name):
        t = S.elem_type.get(name)
        return [c for c, _ in S.children(S.ctypes[t])] if t in S.ctypes else []

    total_h = total_n = 0
    for part, ctname in ROOTS.items():
        found, stack, seen = set(), [(part, ctname)], set()
        while stack:
            n, t = stack.pop()
            found.add(n)
            if t not in S.ctypes or t in seen:
                continue
            seen.add(t)
            stack += S.children(S.ctypes[t])
        found -= OUT_OF_SCOPE
        # A container counts as handled when we handle what it holds: we
        # dispatch on <mergeCell>, never on its <mergeCells> wrapper.
        handled = set(imp)
        for _ in range(4):
            for n in list(found):
                k = kids(n)
                if k and any(c in handled for c in k):
                    handled.add(n)
        done = sorted(x for x in found if x in handled)
        gap = sorted(x for x in found if x not in handled)
        total_h += len(done)
        total_n += len(found)
        pct = 100.0 * len(done) / len(found) if found else 100.0
        print(f"{part:<12} {len(done):>3}/{len(found):<4} {pct:5.1f}%")
        if args.list and gap:
            print(f"             unhandled: {', '.join(gap)}")
    print("-" * 34)
    print(f"{'OVERALL':<12} {total_h:>3}/{total_n:<4} {100.0 * total_h / total_n:5.1f}%")


if __name__ == "__main__":
    main()
