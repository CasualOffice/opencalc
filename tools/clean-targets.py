#!/usr/bin/env python3
"""Reclaim build output: every `target/`, agent worktrees, and test artifacts.

This repository builds four separate Cargo workspaces (the root, `fuzz/`,
`desktop/`, and whatever agent worktrees exist), and a full gate run leaves
around 15 GB behind. `cargo clean` alone reaches only the workspace it is run
in, so the other three grow until the disk fills — which has happened mid-run
more than once, and an `ENOSPC` in the middle of a build looks like a code
failure rather than a full disk.

A target directory with a live `cargo` in it is left alone. Another session on
this machine may be building, and deleting its artifacts underneath it turns
somebody else's green run red for a reason they cannot see.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def bytes_free() -> int:
    st = os.statvfs(ROOT)
    return st.f_bavail * st.f_frsize


def gib(n: int) -> str:
    return f"{n / 1024**3:.1f} GiB"


def busy_dirs() -> list[Path]:
    """Working directories of every live cargo process on this machine."""
    out = []
    try:
        pids = subprocess.run(
            ["pgrep", "-f", "cargo"], capture_output=True, text=True, check=False
        ).stdout.split()
    except FileNotFoundError:
        return out
    for pid in pids:
        cwd = subprocess.run(
            ["lsof", "-a", "-p", pid, "-d", "cwd", "-Fn"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
        for line in cwd.splitlines():
            if line.startswith("n"):
                out.append(Path(line[1:]))
    return out


def main() -> int:
    before = bytes_free()
    busy = busy_dirs()

    # Agent worktrees are removed through git, so the worktree list does not
    # keep a stale entry pointing at a directory that is gone.
    worktrees = ROOT / ".claude" / "worktrees"
    if worktrees.is_dir():
        for wt in sorted(worktrees.iterdir()):
            if any(str(b).startswith(str(wt)) for b in busy):
                print(f"  keep  {wt.name} (a cargo build is running in it)")
                continue
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(wt)],
                cwd=ROOT, capture_output=True, check=False,
            )
            if wt.exists():
                shutil.rmtree(wt, ignore_errors=True)
            print(f"  gone  worktree {wt.name}")
        subprocess.run(["git", "worktree", "prune"], cwd=ROOT, check=False)

    targets = [ROOT / "target", ROOT / "fuzz" / "target", ROOT / "desktop" / "target"]
    for t in targets:
        if not t.is_dir():
            continue
        if any(str(b).startswith(str(t.parent)) for b in busy):
            print(f"  keep  {t.relative_to(ROOT)} (a cargo build is running in it)")
            continue
        shutil.rmtree(t, ignore_errors=True)
        print(f"  gone  {t.relative_to(ROOT)}")

    for junk in ("tests/browser/test-results", "tests/browser/playwright-report"):
        p = ROOT / junk
        if p.is_dir():
            shutil.rmtree(p, ignore_errors=True)
            print(f"  gone  {junk}")

    after = bytes_free()
    print(f"\nfreed {gib(max(0, after - before))} — {gib(after)} free")
    return 0


if __name__ == "__main__":
    sys.exit(main())
