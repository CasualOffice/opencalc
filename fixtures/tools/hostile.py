#!/usr/bin/env python3
"""Generate the hostile-workbook fixture for SEC-001.

A workbook is **untrusted input**. Anybody can send one, and opening it is the
whole point of the product — so any path that turns workbook text into markup is
a way to run script in the editor's origin, which is where the document, the
session token and the collaboration socket live.

This file carries the payload in every place the editor later shows workbook
text back to a person:

  * a **defined name's `refersTo`** (the Name Manager, the finding's original
    sink). The name itself is ordinary: the importer correctly refuses one
    containing `<`, so the payload rides in the target, which is free text.
  * a **sheet name**,
  * a **cell value**, which is drawn to canvas but also read back into the
    formula bar and the compatibility report.

None of it is valid in the sense Excel would produce, and all of it is what an
attacker sends. The importer is expected to accept the file — refusing hostile
*text* is not the defence, not building DOM from it is.

Regenerate with:

    python3 fixtures/tools/hostile.py
"""

import pathlib
import shutil
import zipfile

SOURCE = pathlib.Path("fixtures/generated/minimal.xlsx")
OUT = pathlib.Path("fixtures/generated/hostile-names.xlsx")

# `&` first: escaping in the wrong order double-escapes, and a fixture that
# arrives already-neutralised proves nothing.
PAYLOAD = '<img src=x onerror="window.__pwned=1">'
XML_PAYLOAD = (
    PAYLOAD.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")
)

WORKBOOK = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
    ' xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
    f'<sheets><sheet name="{XML_PAYLOAD}" sheetId="1" r:id="rId1"/></sheets>'
    # The *name* carries a syntactically ordinary identifier, because the
    # importer refuses one containing `<` — refusing it is correct, and it also
    # means an attack has to come through a field that survives. `refersTo` is
    # free text as far as this file is concerned, and the Name Manager showed
    # both, so the payload rides there.
    "<definedNames>"
    f'<definedName name="Hostile">{XML_PAYLOAD}</definedName>'
    f'<definedName name="AlsoHostile">Sheet1!$A$1&amp;"{XML_PAYLOAD}"</definedName>'
    "</definedNames>"
    "</workbook>"
)

SHEET = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
    "<sheetData>"
    f'<row r="1"><c r="A1" t="inlineStr"><is><t>{XML_PAYLOAD}</t></is></c></row>'
    "</sheetData>"
    "</worksheet>"
)


def main():
    shutil.copy(SOURCE, OUT)
    # Rewritten rather than appended: a zip may hold two entries with the same
    # name, and which one a reader takes is exactly the ambiguity a fixture must
    # not have.
    with zipfile.ZipFile(SOURCE) as src:
        entries = {n: src.read(n) for n in src.namelist()}
    entries["xl/workbook.xml"] = WORKBOOK.encode()
    entries["xl/worksheets/sheet1.xml"] = SHEET.encode()
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as out:
        for name, data in entries.items():
            out.writestr(name, data)

    # The payload has to survive into the file, unescaped once. A fixture that
    # is already inert is a test that passes for the wrong reason.
    with zipfile.ZipFile(OUT) as check:
        book = check.read("xl/workbook.xml").decode()
    assert XML_PAYLOAD in book, "the payload did not reach the workbook part"
    assert PAYLOAD not in book, "the payload is unescaped in the XML; it would not parse"
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
