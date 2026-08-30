#!/usr/bin/env python3
"""The openable-extension lists must agree across the workspace boundary.

`TAURI-013`. `desktop/` is a separate cargo workspace (`ADR-023`) and therefore
cannot depend on `casual-calc-wasm`, so the list of extensions the application
will open is written out **twice**:

  - `crates/casual-calc-wasm/src/io.rs` — what the file picker offers.
  - `desktop/src/launch.rs`             — what a double-clicked file may be.

Nothing compared them. A format added to one and not the other is not a
crash — both filter through `SessionFormat::for_extension`, so neither list can
over-claim — but it is a format the panel offers and the desktop refuses to
open, or the reverse, with nothing anywhere saying so. That silence is the
defect; the bound on its damage is why the row was P3 rather than P1.

A third list, `desktop/src/dialog.rs`'s `SPREADSHEET_EXTENSIONS`, is documented
as a deliberate pre-report floor — the associations registered with the
operating system, which are *deliberately* fewer than what can be opened. So it
is checked as a **subset**, not for equality: narrowing it is a decision, and
listing something there that cannot be opened at all is not.

Duplication is the right trade here — `ADR-023` says a crate earns its place by
having more than one consumer, and a seven-element array does not. What was
missing was not a crate; it was this check.
"""
from __future__ import annotations

import pathlib
import re
import sys

ARRAY = re.compile(
    r"const\s+(CANDIDATE_EXTENSIONS|SPREADSHEET_EXTENSIONS)\s*:\s*[^=]+=\s*&?\[(.*?)\]",
    re.S,
)


def extensions(path: pathlib.Path, name: str) -> list[str] | None:
    text = path.read_text(encoding="utf-8")
    for m in ARRAY.finditer(text):
        if m.group(1) == name:
            return re.findall(r'"([^"]+)"', m.group(2))
    return None


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    sources = {
        "the file picker": (root / "crates/casual-calc-wasm/src/io.rs", "CANDIDATE_EXTENSIONS"),
        "double-click launch": (root / "desktop/src/launch.rs", "CANDIDATE_EXTENSIONS"),
    }
    lists = {}
    for label, (path, name) in sources.items():
        got = extensions(path, name)
        if got is None:
            print(f"{path.relative_to(root)}: no `{name}` array found.")
            print("If it was renamed, rename it here too — this gate exists because")
            print("nothing else compares these two lists.")
            return 1
        lists[label] = (path.relative_to(root), got)

    (a_path, a), (b_path, b) = lists["the file picker"], lists["double-click launch"]
    if sorted(a) != sorted(b):
        only_a, only_b = sorted(set(a) - set(b)), sorted(set(b) - set(a))
        print("the openable-extension lists disagree across the workspace boundary:")
        print(f"  {a_path}: {a}")
        print(f"  {b_path}: {b}")
        if only_a:
            print(f"  offered by the picker, refused on double-click: {only_a}")
        if only_b:
            print(f"  opened on double-click, absent from the picker: {only_b}")
        print()
        print("`desktop/` cannot depend on the wasm crate (ADR-023), so these are")
        print("written out twice on purpose. Keep them equal, or say in the tracker")
        print("why they should differ.")
        return 1

    floor_path = root / "desktop/src/dialog.rs"
    floor = extensions(floor_path, "SPREADSHEET_EXTENSIONS")
    if floor is None:
        print(f"{floor_path.relative_to(root)}: no `SPREADSHEET_EXTENSIONS` array found.")
        return 1
    extra = sorted(set(floor) - set(a))
    if extra:
        print(f"{floor_path.relative_to(root)}: registers file associations we cannot open: {extra}")
        print()
        print("This list may be *narrower* than what the application opens — that is")
        print("what it is for. It may not name something that is not openable at")
        print("all, which claims a file type from other applications and then fails.")
        return 1

    print(f"openable extensions: {len(a)} agreed across 2 workspaces, "
          f"{len(floor)} registered as associations")
    return 0


if __name__ == "__main__":
    sys.exit(main())
