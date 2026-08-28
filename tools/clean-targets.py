#!/usr/bin/env python3
"""Reclaim build output: every `target/`, including the ones inside worktrees.

This repository builds four separate Cargo workspaces (the root, `fuzz/`,
`desktop/`, and whatever agent worktrees exist), and a full gate run leaves
around 15 GB behind. `cargo clean` alone reaches only the workspace it is run
in, so the other three grow until the disk fills — which has happened mid-run
more than once, and an `ENOSPC` in the middle of a build looks like a code
failure rather than a full disk.

A target directory with a live `cargo` in it is left alone. Another session on
this machine may be building, and deleting its artifacts underneath it turns
somebody else's green run red for a reason they cannot see.

**This never removes a worktree.** An earlier version did, and destroyed three
running agents: the liveness check only sees a running `cargo`, so an agent
that is reading files is indistinguishable from an abandoned tree, and an
`rmtree` fallback went around the lock git had rightly refused on. Build output
always rebuilds; a worktree may hold the only copy of something. Remove one
deliberately, with `git worktree remove`, when you know its agent is done.
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

    # **Worktrees are never removed here, and the `target/` inside them is.**
    #
    # This tool deleted three live agents' worktrees the first time it ran.
    # The guard below only looks for a running `cargo`, and an agent that is
    # reading files has none — so a worker that had just started looked
    # exactly like an abandoned tree. Worse, git had those worktrees *locked*,
    # `git worktree remove` refused as it should, and an `rmtree` fallback
    # went round the refusal. A lock is git saying do not touch; a tool that
    # overrides it is not cleaning up, it is destroying work.
    #
    # So the two jobs are separated by what they cost to get wrong. Deleting
    # build output is always recoverable — it rebuilds. Deleting a worktree
    # can destroy work that exists nowhere else. This tool now does only the
    # first, in the main checkout and inside every worktree alike, which is
    # where nearly all the space was anyway.
    #
    # Removing a finished agent's worktree is a decision for whoever knows the
    # agent is finished: `git worktree remove .claude/worktrees/agent-<id>`.
    worktrees = ROOT / ".claude" / "worktrees"
    if worktrees.is_dir():
        for wt in sorted(worktrees.iterdir()):
            if not wt.is_dir():
                continue
            for t in (wt / "target", wt / "desktop" / "target", wt / "fuzz" / "target"):
                if not t.is_dir():
                    continue
                if any(str(b).startswith(str(t.parent)) for b in busy):
                    print(f"  keep  {t.relative_to(ROOT)} (a cargo build is running in it)")
                    continue
                shutil.rmtree(t, ignore_errors=True)
                print(f"  gone  {t.relative_to(ROOT)}")

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
