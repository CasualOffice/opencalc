#!/usr/bin/env python3
"""Score formula-function coverage against ECMA-376 Part 1 §18.17.7.

The schema types `<f>` as a plain string, so function coverage cannot come from
the XSD; the function library is specified in prose instead. `spec-functions.txt`
is extracted from the standard's §18.17.7 section headings — see the header of
that file for how. Usage: functions.py <spec-functions.txt>
"""
import pathlib, re, sys

root = pathlib.Path(__file__).resolve().parents[2]
# Skip the provenance header, or the comment lines inflate the denominator.
spec = {l.strip() for l in open(sys.argv[1]) if l.strip() and not l.startswith("#")}
src = (root / "crates/casual-calc-eval/src/functions.rs").read_text()
ours = set(re.findall(r'"([A-Z][A-Z0-9._]*)"\s*=>', src))
ours |= set(re.findall(r'\("([A-Z][A-Z0-9._]*)"\s*,', src))

covered = ours & spec
print(f"spec functions   {len(spec)}")
print(f"implemented      {len(ours)}")
print(f"covered          {len(covered)}  ({100 * len(covered) / len(spec):.1f}%)")
print(f"\nbeyond the spec (later Excel): {', '.join(sorted(ours - spec)) or '(none)'}")
missing = sorted(spec - ours)
print(f"\nmissing ({len(missing)}):")
for i in range(0, len(missing), 12):
    print("  " + ", ".join(missing[i:i + 12]))
