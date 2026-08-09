#!/usr/bin/env python3
"""How *deeply* a construct is supported, not just whether it round-trips.

The structural score counts a construct as handled when it survives import and
export. That is the right measure for "does this file come back intact", and the
wrong one for "does this product support the feature" — those are different
claims, and one percentage cannot make both.

This classifies every modelled field into three depths:

  CARRIED   a verbatim attribute map. Round-trips; nothing reads it. We are a
            faithful courier and nothing more.
  ROUND-TRIP a real modelled field that import and export understand, but which
            no renderer or editor code reads. The file survives; the user never
            sees or edits it.
  LIVE      something outside import/export reads it — it reaches the screen or
            an edit path.

None of these is dishonest on its own. Reporting only the total would be.
"""
import pathlib, re, sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[2]
# Anything that is not the file layer. If one of these reads a field, the field
# does something for a user.
CONSUMERS = [
    "crates/casual-calc-render/src",
    "crates/casual-calc-layout/src",
    "crates/casual-calc-eval/src",
    "crates/casual-calc-transaction/src",
    "crates/casual-calc-wasm/src",
    "webapp/editor.js",
]


def consumer_text():
    out = []
    for path in CONSUMERS:
        p = ROOT / path
        if p.is_file():
            out.append(p.read_text())
        elif p.is_dir():
            for f in p.rglob("*.rs"):
                out.append(f.read_text())
    return "\n".join(out)


def main():
    text = consumer_text()
    fields = []
    for src in (ROOT / "crates/casual-calc-model/src").glob("*.rs"):
        body = src.read_text()
        # The type may itself contain commas (`BTreeMap<String, String>`), so
        # match to the end of the line rather than to the first comma.
        for m in re.finditer(r"^\s*pub (\w+): (.+?),?\s*$", body, re.M):
            name, ty = m.group(1), m.group(2).strip()
            carried = "BTreeMap<String, String>" in ty or "RetainedRef" in ty
            # Field *access*, not a bare word: names like `attrs`, `range` and
            # `style` appear all over unrelated code, and matching those would
            # score half the model LIVE on a coincidence.
            live = re.search(rf"\.{re.escape(name)}\b", text) is not None
            depth = "CARRIED" if carried else ("LIVE" if live else "ROUND-TRIP")
            fields.append((depth, src.stem, name, ty))

    counts = defaultdict(int)
    for depth, *_ in fields:
        counts[depth] += 1
    total = len(fields)
    print(f"{'DEPTH':<11}{'COUNT':>6}  {'SHARE':>7}")
    for depth in ("LIVE", "ROUND-TRIP", "CARRIED"):
        print(f"{depth:<11}{counts[depth]:>6}  {counts[depth] / total * 100:>6.1f}%")
    print(f"{'TOTAL':<11}{total:>6}")

    if "--list" in sys.argv:
        for want in ("ROUND-TRIP", "CARRIED"):
            print(f"\n=== {want} ===")
            for depth, module, name, _ in sorted(fields):
                if depth == want:
                    print(f"  {module}::{name}")


if __name__ == "__main__":
    main()
