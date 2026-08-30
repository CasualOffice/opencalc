#!/usr/bin/env python3
"""The gates are tested, and no gate keeps a private copy of a shared pattern.

Eighteen gates guard this repository and, until this file, **nothing guarded
them**. That is not a theoretical gap. `check-tracker-shape` and
`check-tracker-freshness` each re-derived "what a tracker row looks like" from
the ids in front of them and landed on `[A-Z][A-Z0-9]*(?:-[A-Z]+)*-[0-9]+`:
upper case only, digits at the end. `check-tracker-ids`, written earlier, had
the right pattern all along.

The cost was **47 invisible rows** — `UX-P08a`, `UX-A11Y-01`, `BUG-FREEZEPANE`,
`M11-1a`, `P1A-003b`, `REVIEW-FIXES`. Four were `Done` rows sitting in the live
tracker that the freshness gate reported as "0 closed" and a sweep of closed
rows walked straight past; 43 were archive rows the shape gate never checked.
Both gates printed a confident green, because a gate that cannot see a row
cannot fail on it. **That is the failure mode worth a gate of its own: not a
wrong answer, a narrower question.**

So two checks here.

1. **The shared definition sees the ids the trackers really hold**, including
   the irregular ones. Drawn from real ids, not invented ones: a fixture of
   `ABC-01` would have passed under the narrow regex too, and proved nothing.
2. **No other gate defines its own row-id pattern.** This is the part that
   stops the class recurring. Fixing the two regexes without this would leave
   the next gate free to guess again, and the next guess is as likely to be
   narrow as this one was.
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from tracker_rows import ID_ROW, cells, rows  # noqa: E402

TOOLS = pathlib.Path("tools")

# Real ids, each an id shape that the narrow regex could not see. The comment
# on each is the reason it exists, so a future edit cannot quietly drop one for
# looking redundant.
MUST_SEE = [
    ("FID-19", "the regular shape, which every version handled"),
    ("UX-NAV-01", "three parts"),
    ("UX-P08a", "a lower-case suffix after the number"),
    ("UX-A11Y-01", "digits inside a part, not only at the end"),
    ("BUG-FREEZEPANE", "no number anywhere"),
    ("M11-1a", "digits in the first part and a suffix on the second"),
    ("P1A-003b", "both at once"),
    ("REVIEW-FIXES", "two words, no number"),
]

# A row's first cell is an id. These are not rows, and a pattern loose enough to
# match them would report a table's own header as a row appearing before itself.
MUST_NOT_SEE = [
    ("ID", "the header's first cell"),
    ("Note", "a plain word"),
    ("---", "the separator"),
]

# A regex literal that anchors to the start of a table row and then names an
# id-shaped thing. Finding one outside `tracker_rows` means a gate has taken a
# private copy of the shared answer.
#
# Matched on the *normalised* line rather than one exact spelling. The first
# version of this check looked for a single literal form and a mutation written
# with a doubled backslash slipped past it — which is the same mistake as the
# bug it exists to catch, one level up: a check that answers a narrower question
# than it prints. Backslashes and quote style are stripped before matching, so
# `r"^\|..."`, `r'^\\|...'` and the rest all read alike.
COMPILE = re.compile(r"re\.compile\(")


def looks_like_a_row_pattern(line: str) -> bool:
    """A regex anchoring at a row start and naming upper-case id characters.

    `check-doc-references` also anchors `^|` — for `docs/NN` numbers, with no
    `A-Z` in it — so requiring both is what separates a row-id pattern from the
    other legitimate anchors.
    """
    if not COMPILE.search(line):
        return False
    bare = line.replace("\\", "")
    return "^|" in bare and "A-Z" in bare


def main() -> int:
    problems = []

    for rid, why in MUST_SEE:
        if not ID_ROW.match(f"| {rid} | Title | Open | P2 |"):
            problems.append(
                f"the shared row pattern cannot see `{rid}` ({why}) — a row it "
                f"cannot see is a row no gate can fail on"
            )
    for text, why in MUST_NOT_SEE:
        if ID_ROW.match(f"| {text} | Title | Open | P2 |"):
            problems.append(
                f"the shared row pattern matches `{text}` ({why}), which is not a row"
            )

    # `\|` is an escape, not a cell boundary — markdown says so, and a gate that
    # disagreed would demand an escape and then reject the escaped row.
    escaped = cells(r"| ID-01 | one \| two | Open |")
    if len(escaped) != 3 or escaped[1] != r"one \| two":
        problems.append(f"`cells()` does not honour a `\\|` escape: {escaped!r}")

    # A fenced block holds *examples* of the row format; `docs/14` opens with
    # one. Counting those made a gate report the documentation of the shape as a
    # violation of it.
    fixture = "| REAL-01 | a | Open |\n```\n| FAKE-01 | b | Open |\n```\n"
    seen = [rid for _, rid, _ in rows(fixture)]
    if seen != ["REAL-01"]:
        problems.append(f"`rows()` does not skip fenced examples: saw {seen}")

    # **A gate must survive a workflow whose matrix is an expression**
    # (`CI-033`). `ci.yml`'s `desktop` job builds its `include` with
    # `fromJSON(...)`, so YAML hands back a *string* where a list of dicts used
    # to be. `check-workflow-bashisms.py` walked it as a list and died with
    # `'str' object has no attribute 'values'`, taking main red — the two
    # changes were mine, made an hour apart, and each was verified alone.
    #
    # The three shapes below are the ones a real workflow produces. A crash
    # here is the failure being guarded: a gate that raises does not report a
    # finding, it reports nothing, and CI shows a red job with a traceback
    # instead of an answer.
    try:
        import importlib.util

        spec = importlib.util.spec_from_file_location(
            "bashisms", TOOLS / "check-workflow-bashisms.py"
        )
        bashisms = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(bashisms)
        for shape in (
            {"runs-on": "${{ matrix.os }}", "strategy": {"matrix": "${{ fromJSON(x) }}"}},
            {"runs-on": "${{ matrix.os }}", "strategy": {"matrix": {"include": "${{ fromJSON(x) }}"}}},
            {"runs-on": "${{ matrix.os }}", "strategy": {"matrix": {"include": ["macos-latest"]}}},
        ):
            bashisms.runners_for(shape)
    except Exception as exc:  # noqa: BLE001 — any raise at all is the finding
        problems.append(
            f"`check-workflow-bashisms.runners_for` raises on an expression-valued "
            f"matrix: {exc!r}. A gate that raises reports nothing, not nothing-wrong"
        )

    # **A gate named only in a comment is not wired** (`CI-032`).
    #
    # `check-doc-claims.py` used to answer this with a substring search over the
    # concatenated text of every workflow, so a `#` comment satisfied it — and
    # `check-workflow-bashisms.py` shipped in exactly that state: named by a
    # row, explained by a comment, run by nothing, and reported as wired.
    #
    # The fixture is a whole workflow rather than a line, because the rule now
    # depends on the YAML structure — which step, and which part of it.
    try:
        import importlib.util

        spec = importlib.util.spec_from_file_location(
            "doc_claims", TOOLS / "check-doc-claims.py"
        )
        claims = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(claims)
        import yaml as _yaml

        def wired(workflow_text):
            doc = _yaml.safe_load(workflow_text)
            out = set()
            for job in (doc.get("jobs") or {}).values():
                for step in job.get("steps") or []:
                    body = (step or {}).get("run")
                    if not isinstance(body, str):
                        continue
                    for line in body.splitlines():
                        if line.strip().startswith("#"):
                            continue
                        out.update(claims.SCRIPT.findall(line))
            return out

        commented = """
jobs:
  policy:
    steps:
      # tools/check-example.py explains what this asserts
      - run: echo hello
"""
        executed = """
jobs:
  policy:
    steps:
      - run: python3 tools/check-example.py
"""
        if "check-example.py" in wired(commented):
            problems.append(
                "`check-doc-claims` counts a gate named only in a workflow comment "
                "as wired — the hole `CI-032` records, where a promise that CI runs "
                "a check was kept by a sentence saying CI runs it"
            )
        if "check-example.py" not in wired(executed):
            problems.append(
                "`check-doc-claims` does not see a gate that a `run:` step executes, "
                "so it would report every wired gate as missing"
            )
    except Exception as exc:  # noqa: BLE001
        problems.append(f"`check-doc-claims` could not be exercised: {exc!r}")

    copies = []
    for gate in sorted(TOOLS.glob("check-*.py")):
        if gate.name == "check-gate-selftest.py":
            continue
        lines = gate.read_text(encoding="utf-8").splitlines()
        if any(looks_like_a_row_pattern(line) for line in lines):
            copies.append(gate.name)
    if copies:
        problems.append(
            f"{', '.join(copies)} define their own row-id pattern instead of "
            f"importing `tracker_rows.ID_ROW`. Two gates already did this and "
            f"guessed narrower than the trackers; each copy is another chance "
            f"for a gate to answer a smaller question than it prints"
        )

    if problems:
        print("the gates' own checks do not pass:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    print(
        f"gate self-test: {len(MUST_SEE)} id shapes seen, "
        f"{len(MUST_NOT_SEE)} rejected, one shared row definition"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
