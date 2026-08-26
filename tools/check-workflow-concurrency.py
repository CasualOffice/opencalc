#!/usr/bin/env python3
"""A workflow that gates main never cancels its own run on main.

Nineteen of forty consecutive `ci` runs on main were `cancelled`, so for months
"is main green?" had no answer about half the time. Nothing was broken and
nothing failed — `cancel-in-progress: true` applied to every ref alike, and a
queued request for the same group discarded the run that was the only evidence
main still built.

The asymmetry the config missed: a pull-request run is superseded the instant
its branch moves, and cancelling it saves real time while destroying nothing
anyone wanted. A push to main is the reverse. There is exactly one per merge,
nobody is waiting on it, and its result is the record. Cancelling that trades
the record for a few runner-minutes.

This asserts the asymmetry survives, because its failure mode is not breaking
loudly — it is somebody simplifying the expression back to `true` in a year, and
a main that is silently unverified again. A red main that means nothing is what
teaches people to ignore a red main that means something.
"""

import pathlib
import re
import sys

WORKFLOWS = pathlib.Path(".github/workflows")

# Workflows that gate main but whose run is not evidence about main. Named with
# the reason, so the exemption is a decision on the record rather than an
# omission — the same rule check-ci-retry.py follows.
EXEMPT = {
    "pages.yml": "a deploy, not a gate: a superseded deploy is genuinely worthless,"
    " because the run that cancelled it publishes newer content to the same place",
}


def gates_main(text):
    """True when the workflow runs on a push to main."""
    push = re.search(r"^on:\n(?:.*\n)*?  push:\n((?:    .*\n)+)", text, re.M)
    return bool(push and "main" in push.group(1))


def concurrency(text):
    """The workflow's `cancel-in-progress` value, or None if it sets no group."""
    block = re.search(r"^concurrency:\n((?:  .*\n)+)", text, re.M)
    if not block:
        return None
    found = re.search(r"^  cancel-in-progress: *(.+?) *$", block.group(1), re.M)
    return found.group(1) if found else "false"


def main():
    problems = []
    checked = 0
    for path in sorted(WORKFLOWS.glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        if not gates_main(text) or path.name in EXEMPT:
            continue
        setting = concurrency(text)
        if setting is None:
            continue
        checked += 1
        # Either it never cancels, or it excludes main by name. An expression
        # that merely mentions `github.ref` is not enough; it has to say main.
        exempts_main = "refs/heads/main" in setting
        if setting.strip() == "true" or (setting.strip() != "false" and not exempts_main):
            problems.append(
                f"{path.name}: gates main but cancel-in-progress is `{setting}`.\n"
                f"    A push to main is one run per merge and it is the record of\n"
                f"    whether main is releasable. Exclude main:\n"
                f"      cancel-in-progress: ${{{{ github.ref != 'refs/heads/main' }}}}"
            )

    if problems:
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1
    if not checked:
        print("no workflow both gates main and sets a concurrency group", file=sys.stderr)
        return 1
    print(f"workflow concurrency: {checked} main-gating workflow(s) keep their main run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
