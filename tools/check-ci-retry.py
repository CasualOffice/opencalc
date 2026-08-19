#!/usr/bin/env python3
"""Every CI job that talks to the crates.io registry warms it with retries first.

A run failed with an SSL connect error 0.3 seconds after starting, having
compiled nothing, and passed unchanged on re-run. That is not a code defect and
no amount of reviewing the code would have found it — but it reds main, and a
red main that means nothing is the thing that makes a red main that means
something get ignored.

`.github/actions/cargo-warmup` absorbs the blip. This asserts every cargo job
uses it, because the failure mode of a fix like that is not breaking — it is a
new job added six months from now that quietly does not have it, and a flake
nobody can explain because the other twelve jobs are fine.

Jobs that run cargo somewhere this action cannot reach are named below with the
reason, so the exemption is a decision on the record rather than an omission.
"""

import re
import sys
import pathlib

WORKFLOW = pathlib.Path(".github/workflows/ci.yml")
WARMUP = "./.github/actions/cargo-warmup"

# Jobs that invoke cargo where a host-side warm-up cannot help.
EXEMPT = {
    "docker-build": "cargo runs inside the image build, with its own network",
    "dependency-policy": "`cargo install` fetches tools, not this workspace's lockfile",
}


def jobs(text):
    """Split the workflow into (name, body) at two-space indented keys."""
    body = text.split("\njobs:\n", 1)[1]
    starts = [(m.group(1), m.start()) for m in re.finditer(r"^  ([a-z][a-z0-9-]*):$", body, re.M)]
    for i, (name, at) in enumerate(starts):
        end = starts[i + 1][1] if i + 1 < len(starts) else len(body)
        yield name, body[at:end]


def main():
    if not WORKFLOW.exists():
        print(f"{WORKFLOW} is missing", file=sys.stderr)
        return 1

    text = WORKFLOW.read_text()
    action = pathlib.Path(".github/actions/cargo-warmup/action.yml")
    if not action.exists():
        print(f"{action} is missing, so no job can warm the registry", file=sys.stderr)
        return 1

    # The retry has to actually loop. A warm-up that tries once is the inert
    # fix this gate exists to prevent.
    source = action.read_text()
    if "for attempt in" not in source or "sleep" not in source:
        print(f"{action} does not retry; a single fetch is not a warm-up", file=sys.stderr)
        return 1

    missing, checked = [], 0
    for name, body in jobs(text):
        # `cargo` as a command, not the word inside a comment or a path.
        if not re.search(r"(^|[\s|&;(])cargo\s", body, re.M):
            continue
        if name in EXEMPT:
            continue
        checked += 1
        if WARMUP not in body:
            missing.append(name)

    if missing:
        print("these CI jobs run cargo without warming the registry first:", file=sys.stderr)
        for name in missing:
            print(f"  {name}", file=sys.stderr)
        print(f"\nadd:\n      - uses: {WARMUP}", file=sys.stderr)
        print("or add the job to EXEMPT in this file, with the reason.", file=sys.stderr)
        return 1

    print(f"registry warm-up: {checked} cargo jobs covered, {len(EXEMPT)} exempt")
    return 0


if __name__ == "__main__":
    sys.exit(main())
