#!/usr/bin/env python3
"""Every tracker id means exactly one thing.

The trackers are cross-referenced from commit messages, pull requests and each
other, so an id is only useful if it is unique. It stopped being unique twice,
in two different ways, and neither was noticed by a person:

  * `docs/14` and the since-deleted `docs/52` ran independent `FID-01..FID-25` series for
    different work, colliding across eighteen numbers. `FID-13` was both "a
    sheet name that is not an identifier is written unquoted" and "`sheetView`:
    rightToLeft/showFormulas", so `git log --grep FID-13` returned two
    unrelated changes.

  * Rows were appended to `docs/14` without checking the ids already in it, so
    `UX-GRID-02`, `UX-CLIP-01`, `UX-B04` and `UX-B05` each named two different
    defects — while the same file states that ids are never reused.

Both were found by an audit rather than by review, which is the argument for
checking it here. Reviewing a diff shows the row being added; it does not show
the row three hundred lines up that already has that id.

Run with no arguments; exits non-zero and names every clash.
"""

from __future__ import annotations

import pathlib
import re
import sys
from collections import defaultdict

# The documents that carry `| ID | ...` rows other things cite by id.
TRACKERS = [
    "docs/14-EXECUTION-TRACKER.md",
    "docs/53-FEATURE-CORRECTNESS-TRACKER.md",
    "docs/67-REPOSITORY-REMEDIATION-PLAN.md",
]

# A leading table cell holding something id-shaped: `| FID-19 |`, `| UX-NAV-01 |`.
# Deliberately anchored to the start of a row so prose mentioning an id — which
# is a *reference*, not a definition — is not counted as one.
ID_ROW = re.compile(r"^\|\s*([A-Z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)+)\s*\|")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    seen: dict[str, list[str]] = defaultdict(list)

    for name in TRACKERS:
        path = root / name
        if not path.exists():
            # A tracker being consolidated away is not an error; a *missing*
            # one that is still listed would be, but silence here would hide
            # the list going stale, so say so.
            print(f"note: {name} is listed here and does not exist", file=sys.stderr)
            continue
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            match = ID_ROW.match(line)
            if match:
                seen[match.group(1)].append(f"{name}:{number}")

    clashes = {i: where for i, where in seen.items() if len(where) > 1}
    if not clashes:
        print(f"tracker ids: {len(seen)} unique across {len(TRACKERS)} documents")
        return 0

    print("tracker ids are not unique:", file=sys.stderr)
    for identifier, where in sorted(clashes.items()):
        print(f"  {identifier} defined at {', '.join(where)}", file=sys.stderr)
    print(
        "\nAn id names one piece of work. Pick a free one, or a new prefix if it "
        "is a different workstream.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
