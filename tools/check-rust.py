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
    (
        # `fuzz/` is its own Cargo workspace, so `--workspace` above never
        # reaches it and every gate here passed while a fuzz target no longer
        # compiled. `FID-28` changed `transform`'s signature, the whole tree was
        # green, and `fuzz-build` broke on `main` — the one consumer that lives
        # outside the workspace was the one nothing local built.
        #
        # `check`, not `build`: the failure mode is a signature that no longer
        # matches, and type-checking catches that in seconds where a sanitizer
        # build takes minutes.
        "fuzz targets",
        ["cargo", "check", "--manifest-path", "fuzz/Cargo.toml", "--bins"],
        None,
    ),
    (
        # The desktop shell is its own workspace too, for the reason `ADR-023`
        # gives: Tauri's tree is large and every `--workspace` build in CI would
        # otherwise carry it. Gated from the day it exists rather than after it
        # breaks once — `CI-014` is what happens when a separate workspace has
        # no gate, and adding a second one without learning from the first would
        # be the same mistake twice.
        "desktop shell",
        ["cargo", "test", "--manifest-path", "desktop/Cargo.toml"],
        None,
    ),
    (
        # `--all` above reaches only the workspace this runs in, so for two
        # releases the desktop shell had a *test* gate and no format or lint
        # gate at all — and the comment above claiming it was "gated from the
        # day it exists" was a third true. Found by running `cargo fmt` there
        # and watching it rewrite files nobody had touched: `main` was
        # unformatted and every gate was green.
        #
        # This is `CI-014`'s shape a third time. A separate workspace does not
        # inherit anything; each gate has to name it.
        "desktop shell format",
        ["cargo", "fmt", "--manifest-path", "desktop/Cargo.toml", "--", "--check"],
        None,
    ),
    (
        "desktop shell clippy",
        [
            "cargo", "clippy", "--manifest-path", "desktop/Cargo.toml",
            "--all-targets", "--", "-D", "warnings",
        ],
        None,
    ),
]

# The oracle, which is not a compiler gate and belongs here anyway.
#
# `PERF-11` changed how every reference in the engine is stored, passed 1393
# workspace tests and 129 browser tests, and broke `oracle-diff` on `main`. The
# two runners had said yes, and neither of them asks LibreOffice anything — so
# the green meant less than it looked, and a change to formula *semantics* is
# exactly the class only the oracle sees.
#
# Run here when LibreOffice is installed. **Skipped loudly when it is not**: a
# gate that quietly does nothing is how four counters on this project reported
# zero while being scraped (`SRV-05`), and "the oracle did not run" is the one
# thing a reader of this output must not have to infer.
ORACLE = [
    ("oracle: corpus", []),
    ("oracle: package", ["--validate-package"]),
    ("oracle: ods", ["--ods"]),
]


def libreoffice():
    """Where `soffice` is, if it is anywhere."""
    import shutil

    found = shutil.which("soffice") or shutil.which("libreoffice")
    if found:
        return found
    mac = "/Applications/LibreOffice.app/Contents/MacOS/soffice"
    return mac if os.path.exists(mac) else None


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

    # The oracle, after the compiler gates: it is the slowest thing here and
    # there is no point asking LibreOffice about a tree that does not build.
    soffice = libreoffice()
    if quick:
        print("  skip  oracle                       (--quick)")
    elif soffice is None:
        print("  SKIP  oracle                       no LibreOffice here; CI runs it")
    elif failed:
        print("  skip  oracle                       (a compiler gate failed first)")
    else:
        for name, extra in ORACLE:
            done = subprocess.run(
                ["cargo", "run", "-q", "-p", "casual-calc-fidelity", "--", *extra,
                 "--soffice", soffice],
                capture_output=True, text=True, check=False,
            )
            if done.returncode == 0:
                print(f"  ok    {name}")
                continue
            failed.append((name, ["cargo", "run", "-p", "casual-calc-fidelity", *extra], done))
            print(f"  FAIL  {name}")

    if not failed:
        print("\nall gates pass")
        return 0

    print(f"\n{len(failed)} gate(s) failed:", file=sys.stderr)
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
