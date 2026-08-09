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


# Methods too generic, or too broad, to be evidence that a *particular* field
# is reachable.
BULK = {
    "new", "default", "capture", "install", "clone", "len", "is_empty", "iter",
    "get", "set", "push", "insert", "remove", "clear", "from", "into",
}


def model_accessors():
    """Methods on model types, mapped to the fields they touch.

    Scans each `impl` block's method bodies for `self.<field>`, so a field
    reached only through a setter still counts as reachable. Deliberately
    shallow — one level — because that is where the misses were.
    """
    out = defaultdict(set)
    for src in (ROOT / "crates/casual-calc-model/src").glob("*.rs"):
        body = src.read_text()
        file_fields = set(re.findall(r"^\s*pub (\w+): ", body, re.M))
        for m in re.finditer(
            r"pub fn (\w+)\s*(?:<[^>]*>)?\s*\([^)]*\)[^{]*\{", body
        ):
            name = m.group(1)
            # The method body, to its matching brace.
            depth_, i = 0, m.end() - 1
            while i < len(body):
                if body[i] == "{":
                    depth_ += 1
                elif body[i] == "}":
                    depth_ -= 1
                    if depth_ == 0:
                        break
                i += 1
            inner = body[m.end() : i]
            # `self.field` covers accessors; a constructor writes `Self { field
            # }` and names no `self` at all, which is how `ThemeTint::from_tint`
            # — the only way the editor ever sets a tint — stayed invisible.
            touched = set(re.findall(r"self\.(\w+)", inner))
            touched |= {f for f in file_fields if re.search(rf"\b{re.escape(f)}\b", inner)}
            # A constructor or a bulk snapshot touches everything, and its name
            # ("new", "capture", "default") appears in consumer code constantly
            # — counting those would mark the entire model LIVE on a
            # coincidence, which is a worse failure than the one this fixes.
            # A short or single-word method name is not evidence: `tint(`
            # matched an unrelated local helper in the layout crate and marked
            # `tint_micro` reachable. Only a distinctive multi-word name —
            # `set_font_color`, `theme_slot` — is specific enough to stand in
            # for the field itself.
            if name in BULK or len(touched) > 3 or "_" not in name or len(name) < 6:
                continue
            for f in touched:
                out[f].add(name)
    return out


def main():
    text = consumer_text()
    accessors = model_accessors()
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
            # Field access, or a struct literal that initialises the field.
            # `DataValidation { allow_blank, .. }` in the wasm layer is the DV
            # panel's checkbox arriving at the model, and matching only
            # `.allow_blank` missed every field set that way. Still not a bare
            # word: `range` and `style` appear all over unrelated code.
            live = any(
                re.search(pat, text, re.M) is not None
                for pat in (
                    rf"\.{re.escape(name)}\b",
                    rf"^\s*{re.escape(name)}:\s",
                    rf"^\s*{re.escape(name)},\s*$",
                )
            )
            # ...or through a method on the model that owns the field. A
            # consumer that calls `set_font_color(hex, theme)` is using
            # `color_theme` just as surely as one that assigns to it, and the
            # first version of this tool reported three fully-wired theme
            # fields as unreachable because it could not see that. Same class
            # of mistake as inventory.py's, made by the other instrument.
            if not live:
                live = any(
                    re.search(rf"\b{re.escape(fn)}\s*\(", text)
                    for fn in accessors.get(name, ())
                )
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
