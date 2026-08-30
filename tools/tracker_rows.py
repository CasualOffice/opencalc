"""What a tracker row is — defined once, because three gates disagreed.

`check-tracker-ids` was written first and got it right: an id is a word, then
one or more hyphenated parts, and the parts may hold digits and lower case.
That is what the tracker actually contains — `UX-A11Y-01`, `UX-P08a`,
`BUG-FREEZEPANE`, `M11-1a`, `P1A-003b`, `REVIEW-FIXES`.

`check-tracker-shape` and `check-tracker-freshness` were written later and each
re-derived the pattern from the ids in front of them, arriving at
`[A-Z][A-Z0-9]*(?:-[A-Z]+)*-[0-9]+` — upper case only, and a number at the end.
**That is 47 rows they could not see**, including four `Done` rows that a sweep
of the closed rows then walked straight past, and 43 in the archive that were
never shape-checked at all. The narrow pattern was not wrong about the rows it
was written against; it was wrong about the ones it had not met, which is the
half of the id space where an irregular row is most likely to be sitting.

The failure is not the regex. It is that three gates each held a private answer
to the same question, so a row could be a row to one of them and invisible to
the others. Import from here instead.
"""

import re

# `| SOME-ID |` at the start of a line. The header's own first cell is `ID`,
# which has no hyphenated part, so it does not match — a looser pattern reports
# the header as a row appearing before its own header.
ID_ROW = re.compile(r"^\|\s*([A-Z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)+)\s*\|")


def cells(line: str) -> list[str]:
    r"""The row's cells, honouring `\|` as an escape.

    Markdown does not end a cell on an escaped pipe, so neither can this — a
    gate that counted them would demand an escape and then reject the escaped
    row, which is a gate nobody can satisfy.
    """
    return [c.strip() for c in re.split(r"(?<!\\)\|", line.strip().strip("|"))]


def rows(text: str):
    """Yield `(line number, id, line)` for real rows, skipping fenced blocks.

    A fenced block holds *examples* of the row format — `docs/14` opens with one
    under "How a row works". Reading those as rows made a gate report the
    documentation of the shape as a violation of it.
    """
    fenced = False
    for number, line in enumerate(text.splitlines(), 1):
        if line.startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        match = ID_ROW.match(line)
        if match:
            yield number, match.group(1), line
