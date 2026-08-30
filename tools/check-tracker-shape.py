#!/usr/bin/env python3
"""Every tracker row has the columns its table's header declares.

A markdown table row is split on `|`, so an **unescaped pipe inside prose adds a
column** and everything after it shifts. Nothing renders visibly wrong — the
table still draws — but anything reading a column by position now reads the
wrong cell.

That is not hypothetical. `A11Y-01` carried a trailing `|`, giving it a seventh
column, and the evidence a reader (or a script) would find at the end was the
empty one. The row had been shipped, tested and mutation-verified, and read as
having no evidence at all. A row missing a column is the same fault the other
way round: `UX-CUT-04` has five where its table declares six, so its evidence
column *is* its description.

The width comes from each table's own header, not from a number here, so a
file may hold several tables with different shapes — `14a` does — and adding a
column to one is not this gate's business.
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from tracker_rows import ID_ROW, cells  # noqa: E402

ROOT = pathlib.Path(".")
TRACKERS = [
    "docs/14-EXECUTION-TRACKER.md",
    "docs/14a-ARCHIVE-CLOSED-WORK.md",
    "docs/53-FEATURE-CORRECTNESS-TRACKER.md",
]
SEPARATOR = re.compile(r"^\|[\s:|-]+\|\s*$")


def width(line: str) -> int:
    return len(cells(line))


def main() -> int:
    problems = []
    skipped = []
    checked = 0
    for name in TRACKERS:
        path = ROOT / name
        if not path.is_file():
            continue

        # (line number, id, width, the width its own table declared or None)
        #
        # Walked line by line rather than through `tracker_rows.rows()`,
        # because a row's declared width comes from the separator *above* it and
        # that ordering has to be preserved. The row pattern is still the shared
        # one — this gate used to carry a private, narrower copy and could not
        # see 47 rows because of it.
        found: list[tuple[int, str, int, int | None]] = []
        declared = None
        fenced = False
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            # A fenced block holds *examples* of the row format — `docs/14`
            # opens with one under "How a row works". Reading those as rows
            # made this gate report the documentation of the shape as a
            # violation of it.
            if line.startswith("```"):
                fenced = not fenced
                continue
            if fenced:
                continue
            if SEPARATOR.match(line):
                declared = width(line)
                continue
            match = ID_ROW.match(line)
            if match:
                found.append((number, match.group(1), width(line), declared))

        if not found:
            continue

        # **A table with no header is not checked, and that is said out loud.**
        #
        # `14a` has none — it simply starts with rows — and its widths are not
        # uniform. Inferring the shape from the majority would flag 48 rows
        # this gate cannot prove are wrong: a majority is evidence, not a
        # specification. Giving that table a header is what would make it
        # checkable, and that is a change to the document, not to this gate.
        unheadered = sum(1 for _, _, _, dec in found if dec is None)
        if unheadered:
            skipped.append(f"{name}: {unheadered} row(s) in a table with no header")

        for number, rid, wide, dec in found:
            if dec is None:
                continue
            checked += 1
            if wide == dec:
                continue
            more = (
                "an unescaped `|` in the prose" if wide > dec else "a missing column"
            )
            problems.append(
                f"{name}:{number}: {rid} has {wide} columns where its table "
                f"declares {dec} — most likely {more}. Every column after the "
                f"break shifts, so anything reading one by position reads the "
                f"wrong cell; escape the pipe as `\\|`"
            )

    if problems:
        print("tracker rows whose shape does not match their table:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nThe table still renders, which is what makes this quiet: a shifted "
            "row reads as having no evidence, or as having its description where "
            "its evidence should be.",
            file=sys.stderr,
        )
        return 1

    note = f" ({'; '.join(skipped)} — not checked)" if skipped else ""
    print(
        f"tracker shape: {checked} rows across {len(TRACKERS)} documents, "
        f"every row matches its table{note}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
