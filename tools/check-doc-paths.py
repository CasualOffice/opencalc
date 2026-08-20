#!/usr/bin/env python3
"""Every code path a document names exists.

`DOC-025` carries a figure of "142 stale claims" with no list, no tool and no
artifact behind it — the string occurs once in the repository and `git log -S`
shows the consolidation commit introduced it, not an audit. A backlog nobody
can enumerate cannot be worked down, only carried.

So here is one thing that *is* enumerable. A document naming
`sdk/examples/host-toolbar` is making a checkable claim, and an integrator who
follows it and finds nothing has been misled by prose that read as fact. The
first run of this found 49 such paths and 3 wrong: two were name drift — the
thing exists under another name — and one was a sample promised and never
built, which became `SDK-010`.

**What it deliberately does not do is treat every path as a claim.** `CI-007`
names `fuzz/.cargo/` as the fix it *rejected* — "would fix today and drift
tomorrow" — and a naive check calls that a stale reference. A path may be
mentioned as an alternative, a warning, or a thing that must not exist. Those
live in `NAMED_BUT_ABSENT` with their reason, so the exemption is a decision on
the record rather than a hole.
"""

import pathlib
import re
import sys

# A path a document names on purpose, which does not exist on purpose.
NAMED_BUT_ABSENT = {
    "fuzz/.cargo/": "CI-007 names it as the fix it rejected: a second copy of the "
                    "advisory decisions would fix today and drift tomorrow",
    "sdk/examples/host-toolbar": "SDK-010 — promised in docs/55 and never built. "
                                 "Named here so the promise stays visible; delete "
                                 "this line when the sample lands",
}

# `docs` is deliberately absent: `docs/65` is how this project *cites* a
# document, not a path — the file is `docs/65-RUNNING-IT.md`. Treating a
# citation as a path makes every cross-reference a finding, which is how a gate
# earns being switched off.
ROOTS = ("crates", "server", "tools", "webapp", "sdk", "fuzz", "fixtures")
PATH = re.compile(r"`((?:" + "|".join(ROOTS) + r")/[A-Za-z0-9_./-]+)`")

DOCS = sorted(pathlib.Path("docs").glob("*.md")) + [
    p for p in (pathlib.Path("AGENTS.md"), pathlib.Path("CLAUDE.md"), pathlib.Path("README.md"))
    if p.exists()
]


def main():
    named, missing = {}, {}
    for doc in DOCS:
        for match in PATH.finditer(doc.read_text()):
            named.setdefault(match.group(1), set()).add(doc.name)

    for path, where in named.items():
        if path in NAMED_BUT_ABSENT or pathlib.Path(path).exists():
            continue
        # A trailing slash is a directory; without one it may be either.
        if pathlib.Path(path.rstrip("/")).exists():
            continue
        missing[path] = where

    stale = [p for p in NAMED_BUT_ABSENT if pathlib.Path(p.rstrip("/")).exists()]
    if stale:
        print("exemptions that are no longer needed — the path now exists:", file=sys.stderr)
        for path in stale:
            print(f"  {path}", file=sys.stderr)
        return 1

    if missing:
        print("documents naming code that is not there:", file=sys.stderr)
        for path, where in sorted(missing.items()):
            print(f"  {path}  ({', '.join(sorted(where))})", file=sys.stderr)
        print(
            "\nEach of these is one of three things, and they are not treated alike:\n"
            "  * it exists under another name  -> correct the document\n"
            "  * it was promised and never built -> that is a row, not a doc edit\n"
            "  * it is named deliberately and must not exist -> add it to\n"
            "    NAMED_BUT_ABSENT in this file, with the reason",
            file=sys.stderr,
        )
        return 1

    print(f"doc paths: {len(named)} named across {len(DOCS)} documents, all present")
    return 0


if __name__ == "__main__":
    sys.exit(main())
