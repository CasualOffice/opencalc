#!/usr/bin/env python3
"""Every setting the deployment page names must exist in a server.

A documentation page is a promise, and this one is dense with the exact strings
an operator will paste into a compose file. A variable that gets renamed, or a
metric that gets dropped, leaves the page confidently wrong — and unlike prose
that goes vague, a wrong `OPENCALC_*` name costs somebody an afternoon before
they conclude the documentation is lying.

Checked in one direction only. Every name **on the page** must exist in the
code; the code may hold names the page does not mention, because the page is
deliberately the settings an operator changes rather than an exhaustive dump.
"""

import re
import sys
from pathlib import Path

PAGE = Path("webapp/deploy.html")
SERVERS = ["casual-calc-host", "casual-calc-collab-server", "casual-calc-wopi"]


def main() -> int:
    if not PAGE.exists():
        print(f"::error::{PAGE} is missing", file=sys.stderr)
        return 1

    page = PAGE.read_text()
    source = "".join(
        f.read_text()
        for server in SERVERS
        for f in Path("server", server, "src").rglob("*.rs")
    )

    problems = []

    variables = sorted(set(re.findall(r"OPENCALC_[A-Z_0-9]+", page)))
    for name in variables:
        if f'"{name}"' not in source:
            problems.append(f"{PAGE} documents {name}, which no server reads")

    metrics = sorted(set(re.findall(r"opencalc_[a-z_]+", page)))
    for name in metrics:
        if name not in source:
            problems.append(f"{PAGE} documents the metric {name}, which nothing emits")

    # A page that names nothing would pass every check above.
    if len(variables) < 20:
        problems.append(
            f"{PAGE} names only {len(variables)} settings — it used to document far more, "
            "so either a section was lost or this check is reading the wrong file"
        )

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    if problems:
        return 1

    print(f"deployment page: {len(variables)} settings and {len(metrics)} metrics all exist")
    return 0


if __name__ == "__main__":
    sys.exit(main())
