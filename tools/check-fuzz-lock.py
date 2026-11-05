#!/usr/bin/env python3
"""The fuzz workspace's lockfile agrees with its manifests.

`fuzz/` is a **separate cargo workspace with its own lockfile**, and it
path-depends on crates in the main one. So changing a dependency of, say,
`casual-calc-collab-server` silently invalidates `fuzz/Cargo.lock` — nothing in
the main workspace's own `--locked` build notices, because that lockfile is not
the one being checked.

This has now cost two incidents:

  * `DEP-15` found `fuzz/Cargo.lock` still pinning a vulnerable `h2` that CI was
    compiling, because `cargo audit` had only ever read the root lockfile.
  * `DEP-13` added Redis TLS features to the collaboration server and left the
    fuzz lockfile stale. `fuzz-build` failed on **main**, and the warm-up
    action then reported it as a registry outage.

Both were found by a build failing, which is the expensive way. This is the
cheap way: `cargo metadata --locked` on the fuzz manifest refuses if the
lockfile would have to change, so a dependency edit that forgets it fails here
with the command that fixes it — in a job that takes seconds rather than one
that compiles fuzz targets.
"""

import pathlib
import subprocess
import sys

# Every separate workspace with its own lockfile. `desktop/` was added after
# `fuzz/` and had the same exposure with no gate: `casual-calc-eval` gained a
# dependency on `casual-calc-io`, `fuzz/Cargo.lock` was refreshed because this
# gate said so, `desktop/Cargo.lock` was not, and all three desktop builds went
# red on main (2026-11-05, `CI-037`).
MANIFESTS = [pathlib.Path("fuzz/Cargo.toml"), pathlib.Path("desktop/Cargo.toml")]


def check(manifest):
    if not manifest.exists():
        print(f"{manifest} is missing", file=sys.stderr)
        return False
    done = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1",
         "--manifest-path", str(manifest)],
        capture_output=True, text=True, check=False,
    )
    if done.returncode == 0:
        print(f"{manifest.parent}/ lockfile: agrees with its manifest and path dependencies")
        return True
    print(f"the {manifest.parent}/ workspace's lockfile is out of date:", file=sys.stderr)
    for line in done.stderr.strip().splitlines()[:4]:
        print(f"  {line}", file=sys.stderr)
    print(
        f"\n{manifest.parent}/ is its own workspace, so a dependency change anywhere in the\n"
        "main one can invalidate it without the main build noticing. Run:\n"
        "\n"
        f"    cargo update --manifest-path {manifest} --workspace\n"
        "\n"
        f"and commit {manifest.parent}/Cargo.lock in the same change that moved the dependency.",
        file=sys.stderr,
    )
    return False


def main():
    results = [check(m) for m in MANIFESTS]
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
