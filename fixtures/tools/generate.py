#!/usr/bin/env python3
"""Generate OpenCalc's synthetic .xlsx fixtures deterministically.

Bytes are reproducible: every entry uses a fixed timestamp and deflate level, so
re-running produces an identical file (and thus a stable SHA-256 in the
manifest). See ../README.md and docs/23-DOCX... (fixture policy in docs/29).
"""

import hashlib
import json
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # fixtures/
FIXED_DATE = (1980, 1, 1, 0, 0, 0)  # earliest ZIP timestamp — deterministic

MINIMAL_XLSX = {
    "[Content_Types].xml": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '</Types>'
    ),
    "_rels/.rels": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
        '</Relationships>'
    ),
    "xl/workbook.xml": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>'
    ),
    "xl/_rels/workbook.xml.rels": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
        '</Relationships>'
    ),
    "xl/worksheets/sheet1.xml": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        '<sheetData><row r="1"><c r="A1" t="n"><v>42</v></c></row></sheetData></worksheet>'
    ),
}

FIXTURES = {
    "generated/minimal.xlsx": (MINIMAL_XLSX, "generated"),
}


def write_xlsx(path, parts):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for name in sorted(parts):
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, parts[name])


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        h.update(f.read())
    return h.hexdigest()


def main():
    entries = []
    for rel_path, (parts, kind) in sorted(FIXTURES.items()):
        abs_path = os.path.join(ROOT, rel_path)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        write_xlsx(abs_path, parts)
        entries.append({"path": rel_path, "sha256": sha256(abs_path), "kind": kind})

    manifest = {"schemaVersion": 1, "fixtures": entries}
    with open(os.path.join(ROOT, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2, sort_keys=True)
        f.write("\n")
    print("wrote manifest with", len(entries), "fixture(s)")


if __name__ == "__main__":
    main()
