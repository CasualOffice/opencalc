#!/usr/bin/env python3
"""An ADR a finished row was built to is not still "Proposed".

A decision record has a status because the status is load-bearing: `Proposed`
means the team has not committed, and a reader deciding whether to follow it
reads that word before the argument. When the work ships and the status stays
behind, the register says the opposite of the truth — and it does so about
exactly the decisions that turned out to matter enough to build.

`ADR-021` is why this exists. `RND-11` was built to it, shipped, and closed;
the geometry variants it argues for are in the display list and the renderer
draws all three. The register said `Proposed` for the whole of that, and so did
`RND-11`'s own row, in the same sentence that recorded the work as done.

Checked in one direction only, deliberately. A `Done` row naming an ADR must
find it `Accepted` or `Superseded` — the row is the evidence the decision was
taken. The reverse is not checked: an `Accepted` ADR with no finished row is an
ordinary state, since a decision may be made before, or without, anything being
built for it.

`Rejected` is also accepted here, and that is not a loophole: a row can be
*closed by* rejecting an approach — `ADR-019` was rejected and `RND-11` was
built the other way — so what the gate forbids is specifically a decision left
hanging while its consequences shipped.
"""

import pathlib
import re
import sys

REGISTER = pathlib.Path("docs/08-ADR-REGISTER.md")
TRACKERS = [pathlib.Path("docs/14-EXECUTION-TRACKER.md")]

SETTLED = {"Accepted", "Rejected", "Superseded", "Withdrawn"}


def statuses() -> dict[str, str]:
    """Every ADR in the register, and the status column it carries."""
    found = {}
    for line in REGISTER.read_text().splitlines():
        if not line.startswith("| ADR-"):
            continue
        cells = [c.strip() for c in line.split("|")[1:]]
        if len(cells) < 3:
            continue
        found[cells[0]] = cells[2]
    return found


def main() -> int:
    if not REGISTER.exists():
        print(f"::error::{REGISTER} is missing", file=sys.stderr)
        return 1
    known = statuses()
    if not known:
        print("::error::the ADR register lists no ADRs", file=sys.stderr)
        return 1

    problems = []
    rows = 0
    for tracker in TRACKERS:
        if not tracker.exists():
            continue
        for line in tracker.read_text().splitlines():
            if not line.startswith("| "):
                continue
            cells = [c.strip() for c in line.split("|")[1:]]
            if len(cells) < 4 or not re.match(r"^[A-Z]{2,6}-\d+$", cells[0]):
                continue
            row, status, body = cells[0], cells[2], line
            if status != "Done":
                continue
            rows += 1
            for adr in sorted(set(re.findall(r"ADR-\d{3}", body))):
                state = known.get(adr)
                if state is None:
                    problems.append(f"{row} (Done) names {adr}, which the register does not list")
                elif state not in SETTLED:
                    problems.append(
                        f"{row} is Done and was built to {adr}, which the register "
                        f"still calls {state!r}"
                    )

    if problems:
        for p in sorted(set(problems)):
            print(f"::error::{p}", file=sys.stderr)
        return 1

    print(f"adr status: {len(known)} decisions, {rows} finished rows, none left proposed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
