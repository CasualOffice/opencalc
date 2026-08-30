#!/usr/bin/env python3
"""Build the editor's wasm and record what it was built from.

`webapp/pkg` is gitignored, so the build on disk outlives the source it came
from: it survives a branch switch, a `git checkout --`, a stash and a revert.
It then fails in whatever way that *older* source did, which reads as a
regression in whatever you are changing now. That cost one session three
misleading runs (`CI-027`).

The browser suite's preflight refuses a stale build. It needs to know what the
build was made from, and a timestamp cannot tell it: a branch switch rewrites
every source mtime without changing a byte, so an mtime check fires on a build
that is perfectly current. So this records a **content** hash of the sources
beside the build, and the preflight compares that.

Use this instead of calling `wasm-pack` directly. Calling `wasm-pack` yourself
still works — the hash simply goes stale and the preflight falls back to
timestamps, which errs toward asking for a rebuild.
"""

import hashlib
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"
OUT = ROOT / "webapp" / "pkg"
STAMP = OUT / ".oc-source-hash"


def source_hash() -> str:
    """A digest of every Rust source and manifest the engine is built from.

    Sorted by path so it does not depend on directory order, and `target/` is
    excluded because it is build output and enormous.
    """
    digester = hashlib.sha256()
    names = []
    for dirpath, dirnames, filenames in os.walk(CRATES):
        dirnames[:] = [d for d in dirnames if d != "target"]
        for name in filenames:
            if name.endswith(".rs") or name == "Cargo.toml":
                names.append(pathlib.Path(dirpath) / name)
    # Sorted by the **relative path string**, not by `Path`, because the
    # preflight that checks this hash is JavaScript and sorts strings. `Path`
    # sorts by parts, so `crates/a-b/x.rs` and `crates/a/b.rs` come out in the
    # opposite order — a different order is a different digest, and the two
    # sides disagreed on a tree neither of them was wrong about.
    for rel in sorted(str(n.relative_to(ROOT)) for n in names):
        digester.update(rel.encode("utf-8"))
        digester.update((ROOT / rel).read_bytes())
    # The lock file decides which dependency versions are compiled in, so a
    # build made against a different one is a different build.
    lock = ROOT / "Cargo.lock"
    if lock.is_file():
        digester.update(lock.read_bytes())
    return digester.hexdigest()


def main() -> int:
    before = source_hash()
    done = subprocess.run(
        [
            "wasm-pack", "build", "--release", "--target", "web",
            "--out-dir", str(OUT),
        ],
        cwd=CRATES / "casual-calc-wasm",
        check=False,
    )
    if done.returncode != 0:
        return done.returncode

    # Hashed again afterwards and only written if nothing moved during the
    # build. A stamp claiming a source state the build did not see would be
    # worse than no stamp: the preflight trusts it.
    after = source_hash()
    if after != before:
        print(
            "sources changed while building; not recording a source hash. "
            "The preflight will fall back to timestamps.",
            file=sys.stderr,
        )
        STAMP.unlink(missing_ok=True)
        return 0
    STAMP.write_text(after + "\n", encoding="utf-8")
    print(f"wasm built; source hash {after[:16]} recorded in {STAMP.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
