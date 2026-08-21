#!/usr/bin/env python3
"""Run every repository gate, and say which ones failed.

There are thirteen of these now, and they are run one at a time from memory.
That has cost three separate CI failures in a day — a stale fuzz lockfile, a
doc-path exemption that had expired, and a gate run before `git add` so git
could not see the file it was asked about. Every one of them had a gate that
would have caught it, and every one was a gate somebody did not think to run.

So: one command. It runs all of them, does **not** stop at the first failure —
knowing about three problems is worth more than knowing about the first — and
prints the ones that failed at the end where they can be read.

It deliberately does not run `cargo`. The compiler gates are slow, they belong
in their own jobs, and mixing a four-minute build into a two-second check makes
the two-second check something people skip.

That left the slow gates to be run from memory, and `cargo doc` is the one
people forget — it has broken `main` twice, because an unresolved intra-doc
link is an error under rustdoc and nothing else. They have their own runner
now: `tools/check-rust.py`. Two commands, because they have genuinely different
costs, and both named where they can be remembered.
"""

import pathlib
import subprocess
import sys

TOOLS = pathlib.Path("tools")


def main():
    gates = sorted(
        p for p in TOOLS.glob("check-*.py") if p.name != "check-all.py"
    )
    if not gates:
        print("no gates found", file=sys.stderr)
        return 1

    failed = []
    for gate in gates:
        done = subprocess.run(
            [sys.executable, str(gate)], capture_output=True, text=True, check=False
        )
        mark = "ok  " if done.returncode == 0 else "FAIL"
        summary = (done.stdout.strip().splitlines() or [""])[-1]
        print(f"  {mark}  {gate.stem:<22} {summary[:70]}")
        if done.returncode != 0:
            failed.append((gate.stem, done.stderr.strip() or done.stdout.strip()))

    if failed:
        print(f"\n{len(failed)} of {len(gates)} gates failed:\n", file=sys.stderr)
        for name, output in failed:
            print(f"--- {name} ---", file=sys.stderr)
            print(output, file=sys.stderr)
            print(file=sys.stderr)
        return 1

    print(f"\nall {len(gates)} gates pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
