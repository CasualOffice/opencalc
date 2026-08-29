#!/usr/bin/env python3
"""Every document, ADR and tracker row a document cites actually exists.

`check-doc-index.py` guarantees the *map* is whole: every file has a row, every
row resolves, no number is claimed twice. It says nothing about the citations
inside the documents, and those are how this project actually refers to itself
— `docs/19`, `ADR-011`, `PERF-11`, `CI-018`. Three of them were dead:

  * `P2-002` — "incremental dependency graph + dirty propagation" — was closed
    and then **deleted rather than archived** in the 2026-08-18 consolidation.
    `docs/40` and `docs/66` went on citing it, so the one paragraph a reader
    would follow to find out whether the graph was built pointed at nothing.
    `docs/66` still said "Not yet implemented" about code that had shipped.
  * `COL-33` was cited by `UX-CHROME-01` as the row that put presence in the
    menu bar. The COL series runs 32, 35 — there has never been a COL-33.
  * `docs/18`, `33`, `45`, `46` and `52` were retired without the tombstone row
    the index's own numbering discipline promises, so five numbers read as free
    while five documents still cited them.

A dead reference is worse than no reference, because it reads as a decision
recorded somewhere. It also costs a reader the most exactly when they are least
able to notice: following `P2-002` to nothing is indistinguishable from
following it to a file you lack.

Three rules:

  1. **`docs/NN` resolves** — to a file, or to a tombstone row in the index that
     says where the content went. Rule 1 of `check-doc-index` in the other
     direction: a file must have a row; a citation must have one too.
  2. **`ADR-NNN` is in the register.** The register is the only place an ADR's
     existence and status are recorded.
  3. **A tracker id is defined by a tracker row.** Only ids whose prefix a
     tracker actually uses are checked, and the prefixes are derived from the
     trackers rather than listed here — a hand-kept copy of a fact the trackers
     already state is the thing this repository keeps being bitten by.

What is deliberately NOT checked: whether the citation is *apt*. A row that
exists and is the wrong row is a review question, and a gate that guessed would
fail for reasons nobody should act on.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(".")
DOCS_DIR = ROOT / "docs"
INDEX = DOCS_DIR / "00-README.md"
REGISTER = DOCS_DIR / "08-ADR-REGISTER.md"

TRACKERS = [
    "docs/14-EXECUTION-TRACKER.md",
    "docs/14a-ARCHIVE-CLOSED-WORK.md",
    "docs/53-FEATURE-CORRECTNESS-TRACKER.md",
    "docs/67-REPOSITORY-REMEDIATION-PLAN.md",
]

# **The archive is a record of what happened, not a live document.**
#
# `14a` holds closed rows moved out of `14` verbatim, and the commit that
# created it said why the wording is not rewritten: "the archive keeps its
# wording with the dead links stripped, because rewriting a record of what
# happened would falsify it". So it names ids that were live when the row was
# written and are not now — `P2-002` with its link already stripped to `(#)`,
# `FID-01` from a series that was renamed. Checking it would force a choice
# between a red gate and an edited record, and the record wins.
HISTORICAL = {"14a-ARCHIVE-CLOSED-WORK.md"}

# An id a document names on purpose, which is not a row on purpose. Same idiom
# as `NAMED_BUT_ABSENT` in check-doc-paths.py: the exemption is a decision on
# the record rather than a hole.
NAMED_BUT_UNDEFINED = {
    "SEC-01": "CI-016 tells the story of an id it *created and renamed* — "
              "'adding SEC-01 for the row above, then renaming it to SEC-004' — "
              "which is a narrative about a name, not a citation of a row",
    "COL-33": "never a row: the COL series runs 32, 35. `DOC-036` names it in "
              "order to record that `UX-CHROME-01` cited it, which is the finding "
              "rather than a repetition of it",
    "P2-002": "docs/66 names it to say it resolves to nothing: the row was closed "
              "and then deleted rather than archived, so a reader who finds the id "
              "in an older commit message needs somewhere to land. Naming a dead id "
              "in order to retire it is the opposite of citing it",
}

ROW = re.compile(r"^\|\s*([0-9]{2}[a-z]?)\s*\|")
ID_ROW = re.compile(r"^\|\s*([A-Z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)+)\s*\|")
DOC_CITE = re.compile(r"\bdocs/([0-9]{2}[a-z]?)\b")
ADR_CITE = re.compile(r"\bADR-([0-9]{3})\b")


def documents():
    """Every document a citation may live in."""
    return sorted(DOCS_DIR.glob("*.md")) + [
        p
        for p in (ROOT / "AGENTS.md", ROOT / "CLAUDE.md", ROOT / "README.md", ROOT / "CONTRIBUTING.md")
        if p.exists()
    ]


def known_numbers():
    """Numbers with a file, plus numbers the index keeps a tombstone row for."""
    numbers = set()
    for doc in DOCS_DIR.glob("*.md"):
        match = re.match(r"^([0-9]{2}[a-z]?)-", doc.name)
        if match:
            numbers.add(match.group(1))
    for line in INDEX.read_text().splitlines():
        row = ROW.match(line)
        if row:
            numbers.add(row.group(1))
    return numbers


def known_adrs():
    return {
        line.split("|")[1].strip()
        for line in REGISTER.read_text().splitlines()
        if line.startswith("| ADR-")
    }


def known_ids():
    ids = set()
    for name in TRACKERS:
        path = ROOT / name
        if not path.exists():
            print(f"note: {name} is listed here and does not exist", file=sys.stderr)
            continue
        for line in path.read_text().splitlines():
            match = ID_ROW.match(line)
            if match:
                ids.add(match.group(1))
    return ids


def main() -> int:
    for required in (INDEX, REGISTER):
        if not required.exists():
            print(f"::error::{required} is missing — is this running from the repo root?", file=sys.stderr)
            return 1

    numbers = known_numbers()
    adrs = known_adrs()
    ids = known_ids()
    if not adrs or not ids:
        print("::error::the register or the trackers parsed as empty", file=sys.stderr)
        return 1

    # Derived, not listed: a citation is only held to rule 3 when its prefix is
    # one a tracker actually uses, so `RFC-002` in a quotation is not a finding
    # and `PERF-99` is.
    prefixes = {i.rsplit("-", 1)[0] for i in ids} - {"ADR"}
    id_cite = re.compile(
        r"\b((?:" + "|".join(sorted(map(re.escape, prefixes), key=len, reverse=True)) + r")-[0-9]{2,3})\b"
    )

    problems = []
    for doc in documents():
        text = doc.read_text()
        for number in sorted(set(DOC_CITE.findall(text))):
            if number not in numbers:
                problems.append(
                    f"{doc}: cites docs/{number}, which is neither a file nor a "
                    f"tombstone row in {INDEX}"
                )
        for number in sorted(set(ADR_CITE.findall(text))):
            if f"ADR-{number}" not in adrs:
                problems.append(f"{doc}: cites ADR-{number}, which {REGISTER} does not list")
        if doc.name in HISTORICAL:
            continue
        for identifier in sorted(set(id_cite.findall(text))):
            if identifier in ids or identifier in NAMED_BUT_UNDEFINED:
                continue
            problems.append(f"{doc}: cites {identifier}, which no tracker defines")

    stale = [i for i in NAMED_BUT_UNDEFINED if i in ids]
    if stale:
        print("exemptions that are no longer needed — the id is now a row:", file=sys.stderr)
        for identifier in stale:
            print(f"  {identifier}", file=sys.stderr)
        return 1

    if problems:
        print("documents citing things that do not exist:", file=sys.stderr)
        for problem in sorted(problems):
            print(f"::error::{problem}", file=sys.stderr)
        print(
            "\nEach of these is one of four things, and they are not treated alike:\n"
            "  * the target was renamed          -> correct the citation\n"
            "  * the target was retired          -> give it a tombstone row in\n"
            "                                       docs/00-README.md, which is\n"
            "                                       what the numbering discipline\n"
            "                                       already promises\n"
            "  * the target never existed        -> the citation is the defect;\n"
            "                                       drop the id, keep the fact\n"
            "  * it is named deliberately and is\n"
            "    not a row                       -> add it to NAMED_BUT_UNDEFINED\n"
            "                                       in this file, with the reason",
            file=sys.stderr,
        )
        return 1

    print(
        f"doc references: {len(numbers)} numbers, {len(adrs)} ADRs, {len(ids)} tracker ids, "
        "every citation resolves"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
