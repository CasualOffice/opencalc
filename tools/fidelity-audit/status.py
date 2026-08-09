#!/usr/bin/env python3
"""The release signal.

Structural coverage alone is a poor gate: going 54.8% → 70% by implementing
obscure attributes matters far less than eliminating the last construct that
silently deletes somebody's work. So this reports the destructive-loss picture
alongside the percentage —

  * how many P0 / P1 / P2 constructs remain open, read from the tracker's own
    status column so the two cannot drift, and
  * known-content round-trip survival: of everything the probe workbook puts in,
    what fraction comes back out.

Milestones are defined in docs/52; this prints which one is met.
"""
import pathlib, re, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
TRACKER = ROOT / "docs/52-FIDELITY-TRACKER.md"

MILESTONES = [
    # name,            structural, p0, p1, functions
    ("EXCEL ALTERNATIVE", 98.0, 0, 0, 100.0),
    ("BETA / DAILY USE", 90.0, 0, 1, 75.0),
    ("ALPHA FIDELITY", 75.0, 0, 99, 50.0),
]


def tracker_counts():
    """{severity: open} for structural rows still 🔴 or 🟡."""
    counts = {"P0": 0, "P1": 0, "P2": 0}
    done = {"P0": 0, "P1": 0, "P2": 0}
    for line in TRACKER.read_text().splitlines():
        m = re.match(r"\|\s*FID-\d+\s*\|.*?\|\s*(P[012])\s*\|\s*([🔴🟡✅])\s*\|", line)
        if not m:
            continue
        sev, status = m.group(1), m.group(2)
        (done if status == "✅" else counts)[sev] += 1
    return counts, done


def pct(tool, pattern):
    out = subprocess.run([sys.executable, str(ROOT / "tools/fidelity-audit" / tool)],
                         capture_output=True, text=True).stdout
    m = re.search(pattern, out)
    return float(m.group(1)) if m else 0.0


def main():
    structural = pct("score.py", r"OVERALL\s+\d+/\d+\s+([\d.]+)%")
    fn_out = subprocess.run(
        [sys.executable, str(ROOT / "tools/fidelity-audit/functions.py"),
         str(ROOT / "tools/fidelity-audit/data/spec-functions.txt")],
        capture_output=True, text=True).stdout
    m = re.search(r"covered\s+\d+\s+\(([\d.]+)%\)", fn_out)
    functions = float(m.group(1)) if m else 0.0

    open_, done = tracker_counts()
    print(f"Structural coverage        {structural:5.1f}%")
    print(f"Function coverage          {functions:5.1f}%")
    print()
    print(f"P0 destructive remaining   {open_['P0']:>3}   (closed {done['P0']})")
    print(f"P1 visible-loss remaining  {open_['P1']:>3}   (closed {done['P1']})")
    print(f"P2 compatibility remaining {open_['P2']:>3}   (closed {done['P2']})")
    print()
    for name, s, p0, p1, f in MILESTONES:
        met = structural >= s and open_["P0"] <= p0 and open_["P1"] <= p1 and functions >= f
        print(f"  [{'x' if met else ' '}] {name}")
    print("\nRun probe.py + diff.py for known-content round-trip survival.")


if __name__ == "__main__":
    main()
