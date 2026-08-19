#!/usr/bin/env python3
"""Every document in `docs/` is in the index, and the index points at documents.

`docs/00-README.md` calls itself "the map", and AGENTS.md tells every agent to
start there. That makes an unlisted document invisible: `69-COLLABORATIVE-UNDO-
POLICY` through `75-RELATIVE-FORMULA-SHARING-DESIGN` were each added by the work
that needed them, none of them updated the index, and the map stopped at 68 for
weeks without anybody noticing — because nothing about writing a new document
forces a second file to change, and a review diff shows the document being
added, not the index that did not mention it (`DOC-033`).

Three rules, all mechanical:

  1. **Every `docs/*.md` has a row in the index that links to it.** The number
     in the row's first column must be the document's own number, so a row
     cannot be satisfied by a passing mention somewhere else in the table.
  2. **Every relative link in the index resolves.** A renamed document leaves a
     row pointing at nothing, which is the same map failure from the other end.
  3. **No number is claimed by two rows.** Numbers are "stable and never
     reused" per the index's own numbering discipline; two rows with one number
     is that rule broken in the one place it is published.

What is deliberately NOT checked: the *prose* of a row, or which section table
it sits in. A wrong purpose line is a review question, and a gate that guessed
at it would fail for reasons nobody should act on.

Rows that redirect are allowed on purpose: `29`, `31`, `50` and `60` name
numbers whose own file no longer exists and link to where the content went.
That is rule 1 read in one direction only — a file must have a row; a row need
not have a file of its own number.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(".")
DOCS = ROOT / "docs"
INDEX = DOCS / "00-README.md"

# `| 68 | [Clipboard HTML Paste](68-CLIPBOARD-HTML-PASTE.md) | ... |`
ROW = re.compile(r"^\|\s*([0-9]{2}[a-z]?)\s*\|(.*)\|.*$")
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
# `14a-ARCHIVE-CLOSED-WORK.md` -> `14a`
NUMBER = re.compile(r"^([0-9]{2}[a-z]?)-")


def title_of(path: pathlib.Path) -> str:
    """The document's own H1, minus its number prefix — the row to paste."""
    for line in path.read_text().splitlines():
        if line.startswith("# "):
            text = line[2:].strip()
            return re.sub(r"^[0-9]{2}[a-z]? [—-] ", "", text)
    return path.stem


def main() -> int:
    if not INDEX.exists():
        print(f"::error::{INDEX} does not exist — is this running from the repo root?", file=sys.stderr)
        return 1

    linked: dict[str, set[str]] = {}
    seen_at: dict[str, int] = {}
    problems: list[str] = []

    for lineno, line in enumerate(INDEX.read_text().splitlines(), start=1):
        row = ROW.match(line)
        if not row:
            continue
        number, body = row.group(1), row.group(2)
        if number in seen_at:
            problems.append(
                f"{INDEX}:{lineno}: a second row claims number {number} "
                f"(the first is line {seen_at[number]}); numbers are never reused"
            )
        seen_at[number] = lineno
        for target in LINK.findall(body):
            if target.startswith(("http", "#")):
                continue
            target = target.split("#")[0]
            linked.setdefault(number, set()).add(target)
            if not (DOCS / target).exists() and not (ROOT / target).exists():
                problems.append(
                    f"{INDEX}:{lineno}: row {number} links to {target}, which does not exist"
                )

    if not seen_at:
        problems.append(f"{INDEX}: no index rows were found — has the table format changed?")

    missing: list[pathlib.Path] = []
    for doc in sorted(DOCS.glob("*.md")):
        number = NUMBER.match(doc.name)
        if not number:
            problems.append(f"{doc}: does not start with a number, so the index cannot order it")
            continue
        number = number.group(1)
        if doc.name in linked.get(number, set()):
            continue
        # The index itself is the one document that is named rather than linked.
        if doc == INDEX and number in seen_at:
            continue
        missing.append(doc)
        problems.append(f"{doc}: no row in {INDEX} links to it")

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)

    if missing:
        print("\nAdd these rows to the index (section by subject, number order):", file=sys.stderr)
        for doc in missing:
            number = NUMBER.match(doc.name).group(1)
            print(
                f"| {number} | [{title_of(doc)}]({doc.name}) | <one line: why a reader would open it> |",
                file=sys.stderr,
            )

    if problems:
        print(
            f"\n{len(problems)} problem(s). docs/00-README.md is the map every agent is told "
            "to read first; a document missing from it is a document nobody finds. Add the row "
            "in the same commit as the document — do not delete the document's number.",
            file=sys.stderr,
        )
        return 1

    print(
        f"doc index: {len(list(DOCS.glob('*.md')))} documents, {len(seen_at)} index rows, "
        "every document is on the map and every row resolves"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
