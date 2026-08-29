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
*closed by* rejecting an approach, so what the gate forbids is specifically a
decision left hanging while its consequences shipped.

**A second check, for the same failure from the other side.** An ADR's status
lives in the register, and its design note carries a copy of it in the header —
which is the copy a reader actually meets, because they arrive at `78` or `80`
from a link in the work, not by reading `08` first. Both copies were stale:
`78` said "proposed" for a decision the register calls Accepted, and `80` said
"proposed" for one the register records as *"Accepted on the evidence of being
built"*, with `RND-11` shipped and closed against it. Neither the register gate
above nor `check-doc-index` could see it, because each file was internally
consistent.

So: a document that names exactly one ADR in its header **and** declares a
status from the register's own vocabulary must declare the register's status.
The "exactly one" is what keeps it honest — `76` names three ADRs while
declaring a status of its own ("decided — Option C"), and a gate that guessed
which one the status belonged to would fail for a reason nobody should act on.
A header that declares no recognised status word is not a claim and is not
checked.
"""

import pathlib
import re
import sys

REGISTER = pathlib.Path("docs/08-ADR-REGISTER.md")
TRACKERS = [pathlib.Path("docs/14-EXECUTION-TRACKER.md")]

SETTLED = {"Accepted", "Rejected", "Superseded", "Withdrawn"}

# The register's published vocabulary (docs/08 §Status values), plus the two
# the register itself uses in the column. A header word outside this set is
# prose, not a status claim.
VOCABULARY = ("Proposed", "Accepted", "Superseded", "Rejected", "Withdrawn")

# How many lines of a document count as its header. Every status line in
# `docs/` sits in the first few; reading further starts catching the *body*
# discussing other decisions' statuses, which is not a claim about this one.
HEADER_LINES = 12

STATUS_WORD = re.compile(
    r"\*\*Status:?\*?\*?[^\n|]{0,40}?\b(" + "|".join(VOCABULARY) + r")\b", re.I
)

# **Which ADR a note is *about*, as opposed to which it mentions.**
#
# Every note names two to four ADRs in its header — the one it decides, and the
# ones it extends or relates to — so "names an ADR" cannot identify the
# subject. These three forms can, and between them they cover every note in
# `docs/`. Anything matching none of them declares no subject and is not
# checked, which is the safe direction: a note nobody can attribute is a
# review question, not a gate failure.
SUBJECT = (
    re.compile(r"\*\*For\*\*\s+`?(ADR-\d{3})`?"),          # 78, 80, 81
    re.compile(r"^#\s.*?\b(ADR-\d{3})\b", re.M),           # 61-64, H1 is the ADR
    re.compile(r"\*\*Status[^\n]*?\bas (ADR-\d{3})\b"),    # 77: "Accepted as ADR-020"
    re.compile(r"\*\*Status[^\n]*?\((ADR-\d{3})\)"),       # 56-59: "Accepted (ADR-011)"
)


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

    notes = 0
    for doc in sorted(pathlib.Path("docs").glob("*.md")):
        if doc == REGISTER:
            continue
        header = "\n".join(doc.read_text().splitlines()[:HEADER_LINES])
        subject = next(
            (m.group(1) for m in (pattern.search(header) for pattern in SUBJECT) if m), None
        )
        if subject is None:
            continue
        declared = STATUS_WORD.search(header)
        if not declared:
            continue
        notes += 1
        adr = subject
        said, truth = declared.group(1).capitalize(), known.get(adr)
        if truth is None:
            problems.append(f"{doc} declares a status for {adr}, which the register does not list")
        elif said != truth:
            problems.append(
                f"{doc} calls {adr} {said!r}; the register calls it {truth!r}. "
                f"The register is where an ADR's status lives — change it there, "
                f"or fix the note"
            )

    if problems:
        for p in sorted(set(problems)):
            print(f"::error::{p}", file=sys.stderr)
        return 1

    print(
        f"adr status: {len(known)} decisions, {rows} finished rows, {notes} design notes, "
        "none left proposed and no note disagreeing with the register"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
