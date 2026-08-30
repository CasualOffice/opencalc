#!/usr/bin/env python3
"""The live tracker is mostly live work.

`docs/14`'s own preamble says closed rows move to `14a`. That rule was written
in 2026-08, applied once, and then decayed straight back: by 2026-08-30 the
tracker held 285 rows of which **245 were closed**, so the 34 rows anyone
actually needed were 14% of a 382-line file (`DOC-050`).

**No other gate can see this, and that is the point.** A stale row and a live
row are both perfectly well-formed — `check-tracker-shape` checks their columns,
`check-tracker-ids` checks their ids, and a row that has been `Done` for three
months passes both. The decay is only visible as a *ratio*, and nothing was
counting it. That is why this is a gate rather than a paragraph of advice: the
paragraph already existed, in the file it was about.

Closing a row and sweeping it out are deliberately allowed to be separate
commits, so a working margin is left. What is refused is the accumulation.
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from tracker_rows import cells, rows  # noqa: E402

TRACKER = pathlib.Path("docs/14-EXECUTION-TRACKER.md")
ARCHIVE = "docs/14a-ARCHIVE-CLOSED-WORK.md"
CLOSED = {"Done", "Dropped", "Accepted"}

# Below this, closed rows are the ordinary lag between closing a row and
# sweeping it — not debt. The gate stays quiet so it never becomes the reason
# somebody splits a commit.
FLOOR = 20
# Above this share, the open work is no longer what the document is mostly
# about. Half is generous: the state that prompted this gate was 86%.
SHARE = 0.5


def main() -> int:
    if not TRACKER.is_file():
        print(f"{TRACKER} not found", file=sys.stderr)
        return 1

    # The row pattern and the fence skipping are the shared ones. This gate
    # used to carry a private, narrower copy that could not see `UX-P08a` or
    # `UX-A11Y-01`, so it reported "0 closed" over a tracker holding four
    # closed rows — a false green on the one number it exists to report.
    statuses = [
        cells(line)[2] if len(cells(line)) > 2 else ""
        for _, _, line in rows(TRACKER.read_text(encoding="utf-8"))
    ]

    if not statuses:
        print("tracker freshness: no rows to weigh", file=sys.stderr)
        return 1

    closed = sum(1 for s in statuses if s in CLOSED)
    share = closed / len(statuses)
    if closed >= FLOOR and share > SHARE:
        print(
            f"{TRACKER}: {closed} of {len(statuses)} rows are closed "
            f"({share:.0%}) — the live tracker is mostly not live work.",
            file=sys.stderr,
        )
        print(
            f"\nMove the `Done`, `Dropped` and `Accepted` rows to {ARCHIVE}, "
            f"verbatim and without merging ids: they are cited by id from other "
            f"documents and from code, and `check-doc-references` fails on a "
            f"citation to an id no tracker defines.",
            file=sys.stderr,
        )
        return 1

    print(
        f"tracker freshness: {len(statuses) - closed} of {len(statuses)} rows are open work "
        f"({closed} closed, awaiting the sweep)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
