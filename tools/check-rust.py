#!/usr/bin/env python3
"""Run the compiler gates CI runs, and say which ones failed.

`check-all.py` deliberately leaves `cargo` out — the compiler gates are slow,
and mixing a four-minute build into a two-second check makes the two-second
check something people skip. That reasoning is right and it left a hole: the
slow gates were then run *from memory*, and the one people forget is
`cargo doc`.

It has now broken `main` twice. An unresolved intra-doc link is an error under
rustdoc and **nothing else** — `cargo build`, `cargo test` and `cargo clippy`
all pass over it — so a crate can move, take its doc links with it, and be
merged with a dangling reference that only the docs job sees. `RND-11` moved
two modules between crates and did exactly that.

So: the other half of the pair. Two commands rather than one, because they have
genuinely different costs, and both are named where they can be remembered.

    python3 tools/check-all.py     # the repository gates, seconds
    python3 tools/check-rust.py    # the compiler gates, minutes

Like its sibling this does **not** stop at the first failure: knowing about
three problems is worth more than knowing about the first.

`--quick` skips the test run, for the case where the tests were just run and
what is wanted is the lint-and-docs pass over them.
"""

import os
import subprocess
import sys

# The second feature configuration is a second program, and only running it
# finds what differs. Three separate defects have been found this way — a
# format name that disappeared with its decoder (`RND-12`), an alignment fix
# that only touched the arm that does not run (`RND-11`), and `DOC-031`.
GATES = [
    (
        "format",
        ["cargo", "fmt", "--all", "--", "--check"],
        None,
    ),
    (
        "clippy",
        [
            "cargo", "clippy", "--workspace", "--all-targets",
            "--all-features", "--locked", "--", "-D", "warnings",
        ],
        None,
    ),
    (
        "clippy (no default features)",
        [
            "cargo", "clippy", "-p", "casual-calc-render", "--all-targets",
            "--no-default-features", "--", "-D", "warnings",
        ],
        None,
    ),
    (
        "test",
        ["cargo", "test", "--workspace", "--all-features"],
        None,
    ),
    (
        # The one that is forgotten, and the reason this file exists.
        "docs",
        ["cargo", "doc", "--workspace", "--all-features", "--no-deps"],
        {"RUSTDOCFLAGS": "-D warnings"},
    ),
]


def main() -> int:
    quick = "--quick" in sys.argv
    failed = []
    for name, command, extra_env in GATES:
        if quick and name == "test":
            print(f"  skip  {name:<28} (--quick)")
            continue
        env = dict(os.environ)
        if extra_env:
            env.update(extra_env)
        done = subprocess.run(
            command, capture_output=True, text=True, check=False, env=env
        )
        if done.returncode == 0:
            print(f"  ok    {name}")
            continue
        failed.append((name, command, done))
        print(f"  FAIL  {name}")

    if not failed:
        print(f"\nall {len(GATES) - (1 if quick else 0)} compiler gates pass")
        return 0

    print(f"\n{len(failed)} of {len(GATES)} compiler gates failed:", file=sys.stderr)
    for name, command, done in failed:
        print(f"\n--- {name} ---", file=sys.stderr)
        print(f"    {' '.join(command)}", file=sys.stderr)
        # The compiler puts diagnostics on stderr; a failing test run puts the
        # interesting part on stdout. Show whichever has something in it.
        body = (done.stderr or "") + (done.stdout or "")
        lines = [ln for ln in body.splitlines() if ln.strip()]
        for line in lines[-40:]:
            print(line, file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
