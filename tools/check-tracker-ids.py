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

**And the status column is a closed vocabulary**, which `docs/14` states in the
same breath as the id rule: *"`Open`, `WIP`, `Partial`, `Blocked`, `Designed`,
`Done`, `Dropped`. Nothing else."* Nothing enforced it, and `UX-HIDE-01`
carried `Rejected` — a word from the **ADR register's** vocabulary, where a
decision can be rejected, in a table where work cannot. It matters because the
status column is read by machine: `check-adr-status.py` selects on `Done`, and
a row in a fourteenth spelling is a row that gate silently skips.

Only `docs/14` is held to it. The archive keeps whatever a row said when it
closed, and `53` and `67` run their own columns.

Run with no arguments; exits non-zero and names every clash.
"""

from __future__ import annotations

import pathlib
import re
import sys
from collections import defaultdict

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from tracker_rows import ID_ROW  # noqa: E402

# The documents that carry `| ID | ...` rows other things cite by id.
TRACKERS = [
    "docs/14-EXECUTION-TRACKER.md",
    # The archive counts. Closed work keeps its id — that is the whole point of
    # citing one — and an archived id is exactly the id a person reads as free,
    # because it is no longer in the tracker they are looking at. Leaving this
    # out meant the gate against reusing an id could not see the ids most likely
    # to be reused; `SEC-004` was assigned twice before it was added.
    "docs/14a-ARCHIVE-CLOSED-WORK.md",
    "docs/53-FEATURE-CORRECTNESS-TRACKER.md",
    "docs/67-REPOSITORY-REMEDIATION-PLAN.md",
]

# A leading table cell holding something id-shaped: `| FID-19 |`, `| UX-NAV-01 |`.
# Deliberately anchored to the start of a row so prose mentioning an id — which
# is a *reference*, not a definition — is not counted as one.
#
# This pattern was right and now lives in `tracker_rows`, imported above. Two
# later gates re-derived it instead of importing it, guessed narrower, and were
# blind to 47 rows this one always saw. `check-gate-selftest` now refuses a
# second copy.

# The live tracker, and the vocabulary it publishes for its own `St` column.
LIVE = "docs/14-EXECUTION-TRACKER.md"
STATUSES = {"Open", "WIP", "Partial", "Blocked", "Designed", "Done", "Dropped"}
# `| ID | Title | St | ...` — the third cell.
ROW_STATUS = re.compile(ID_ROW.pattern + r"[^|]*\|\s*([^|]*?)\s*\|")


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

    wrong_status: list[str] = []
    live = root / LIVE
    if live.exists():
        for number, line in enumerate(live.read_text().splitlines(), start=1):
            match = ROW_STATUS.match(line)
            if match and match.group(2) not in STATUSES:
                wrong_status.append(
                    f"  {LIVE}:{number}: {match.group(1)} has status "
                    f"{match.group(2)!r}, which is not one of "
                    f"{', '.join(sorted(STATUSES))}"
                )

    clashes = {i: where for i, where in seen.items() if len(where) > 1}
    if not clashes and not wrong_status:
        print(
            f"tracker ids: {len(seen)} unique across {len(TRACKERS)} documents, "
            "every live status in the vocabulary"
        )
        return 0

    if clashes:
        print("tracker ids are not unique:", file=sys.stderr)
        for identifier, where in sorted(clashes.items()):
            print(f"  {identifier} defined at {', '.join(where)}", file=sys.stderr)
        print(
            "\nAn id names one piece of work. Pick a free one, or a new prefix if it "
            "is a different workstream.",
            file=sys.stderr,
        )
    if wrong_status:
        print("statuses outside the vocabulary docs/14 publishes:", file=sys.stderr)
        for problem in wrong_status:
            print(problem, file=sys.stderr)
        print(
            "\nThe column is read by machine — check-adr-status.py selects on "
            "`Done` — so a fourteenth spelling is a row a gate silently skips. "
            "Use the word, or change the vocabulary in docs/14 and here together.",
            file=sys.stderr,
        )
    return 1


if __name__ == "__main__":
    sys.exit(main())
