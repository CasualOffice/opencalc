#!/usr/bin/env python3
"""Generate deliberately broken .xlsx packages.

A script rather than committed binaries, because what makes each file broken has
to be readable. A corpus of opaque bytes tells you a test failed; it does not
tell you what the file was trying to be.

Each output violates exactly one thing, so a failure names the violation. The
contract they exist to check is the one the corpus test asserts: a file must
open, or be refused with a reason. Never a panic, never a hang.

    python3 fixtures/tools/malformed.py
"""
import pathlib
import zipfile

SRC = pathlib.Path("fixtures/generated/minimal.xlsx")
OUT = pathlib.Path("fixtures/corpus/malformed")


def parts() -> dict[str, bytes]:
    with zipfile.ZipFile(SRC) as z:
        return {n: z.read(n) for n in z.namelist()}


def write(name: str, items: dict[str, bytes]) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / name
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for n, data in items.items():
            # Fixed timestamp, so the bytes are stable and the manifest checksum
            # does not change every time this is run.
            info = zipfile.ZipInfo(n, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    print(f"{name}: {path.stat().st_size} bytes")


def main() -> None:
    # A relationship pointing at a part that is not in the package. The workbook
    # exists and names a sheet that does not.
    p = parts()
    p["xl/_rels/workbook.xml.rels"] = p["xl/_rels/workbook.xml.rels"].replace(
        b"worksheets/sheet1.xml", b"worksheets/nowhere.xml"
    )
    write("rel-target-missing.xlsx", p)

    # A cell claiming a shared string that the table does not have. Reading it
    # naively indexes past the end.
    p = parts()
    # A *sibling* cell, not a nested one. The first version of this inserted the
    # new `<c>` inside the existing one, which made the file invalid in a second
    # way and cost the test its meaning: it opened with zero cells, and the
    # reason was the nesting rather than the index.
    p["xl/worksheets/sheet1.xml"] = p["xl/worksheets/sheet1.xml"].replace(
        b"</row>", b'<c r="B1" t="s"><v>9999</v></c></row>', 1
    )
    write("shared-string-out-of-range.xlsx", p)

    # `dimension` disagreeing with the cells actually present: a reader sizing
    # anything from it allocates for a sheet that is not there.
    p = parts()
    p["xl/worksheets/sheet1.xml"] = p["xl/worksheets/sheet1.xml"].replace(
        b'<dimension ref="A1"', b'<dimension ref="A1:XFD1048576"'
    )
    write("dimension-lies.xlsx", p)

    # The content type says one thing and the bytes are another.
    p = parts()
    p["[Content_Types].xml"] = p["[Content_Types].xml"].replace(
        b"spreadsheetml.worksheet+xml", b"image/png"
    )
    write("content-type-wrong.xlsx", p)

    # Truncated XML: the package is intact and a part is not.
    p = parts()
    p["xl/worksheets/sheet1.xml"] = p["xl/worksheets/sheet1.xml"][: len(p["xl/worksheets/sheet1.xml"]) // 2]
    write("part-truncated.xlsx", p)

    # A part name that climbs out of the package. The archive is valid; the name
    # is an instruction to write outside the directory it is extracted into.
    p = parts()
    p["../../escaped.xml"] = b"<x/>"
    write("path-traversal.xlsx", p)

    # No content types at all, which is the one part OPC says must be there.
    p = parts()
    del p["[Content_Types].xml"]
    write("content-types-missing.xlsx", p)


if __name__ == "__main__":
    main()
