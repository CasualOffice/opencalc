#!/usr/bin/env python3
"""The editor's chrome is sized in `rem`; only hairlines stay in `px`.

[docs/103](../docs/103-REM-AND-RESOLUTION.md) (`UX-REM-01`): a chrome sized in
`px` cannot follow the user's default text size, which is how a low-vision user
enlarges text without zooming the whole page. On 2026-11-05 the three chrome
stylesheets held about 1,700 `px` lengths and no `rem` at all.

A conversion that size does not land in one commit, and a rule nobody enforces
until it lands is a rule the next stylesheet edit breaks. So this is a
**ratchet**: each sheet has a recorded count of `px` lengths that are not
hairlines, and

- a count **above** its baseline fails — no new `px` length gets in, starting
  now;
- a count **below** its baseline also fails until the baseline is lowered to
  match — so progress is recorded in the same commit that makes it, and the
  ceiling never sits loose above the tree.

When every baseline is 0 the gate is the full rule of docs/103.

**A hairline** is a length of 2 px or less in a `border*`, `outline*`,
`box-shadow` or `column-rule` declaration. Those stay in `px` on purpose: a
1 px line scaled to 1.25 px is drawn blurred. Everything else — padding,
heights, widths, font sizes, gaps, radii, media queries — counts.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Non-hairline `px` lengths per chrome stylesheet. Lower these as the
# conversion lands; never raise them.
BASELINE = {
    "webapp/editor.css": 1130,
    "webapp/ribbon.css": 205,
    "webapp/ribbon.tokens.css": 18,
}

HAIRLINE_PROPS = re.compile(r"^\s*(border[\w-]*|outline[\w-]*|box-shadow|column-rule[\w-]*)\s*:")
PX = re.compile(r"(?<![\w.-])(-?\d*\.?\d+)px\b")
COMMENT = re.compile(r"/\*.*?\*/", re.S)


def count(path: pathlib.Path) -> int:
    text = COMMENT.sub("", path.read_text(encoding="utf-8"))
    n = 0
    # Declarations and at-rule preludes, split on `;`, `{` and `}` so a
    # one-line rule is read declaration by declaration.
    for part in re.split(r"[;{}]", text):
        hairline_ok = bool(HAIRLINE_PROPS.match(part))
        for m in PX.finditer(part):
            if hairline_ok and abs(float(m.group(1))) <= 2:
                continue
            n += 1
    return n


def main() -> int:
    write = "--print" in sys.argv[1:]
    problems = []
    for name, base in BASELINE.items():
        got = count(ROOT / name)
        if write or base is None:
            print(f"{name}: {got}")
            if base is None:
                problems.append(f"{name} has no baseline recorded (it counts {got})")
            continue
        if got > base:
            problems.append(
                f"{name}: {got} px lengths that are not hairlines, above its baseline of "
                f"{base}. Size chrome in rem (docs/103); only hairlines of 2px or less "
                f"in border/outline/box-shadow stay px."
            )
        elif got < base:
            problems.append(
                f"{name}: {got} px lengths, below its baseline of {base} — lower "
                f"BASELINE in tools/check-css-units.py to {got} in this commit, so the "
                f"ratchet keeps the progress."
            )
    for p in problems:
        print(f"::error::{p}")
    if not problems and not write:
        total = sum(BASELINE.values())
        print(
            f"css units: {len(BASELINE)} chrome sheets at or under their px baseline "
            f"({total} left to convert; 0 is docs/103's rule)"
        )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
