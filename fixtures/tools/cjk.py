#!/usr/bin/env python3
"""Generate the East Asian font fixture for XL-FEAT-140.

Japanese, Simplified Chinese, Traditional Chinese and Korean text, each styled
in the font Excel itself defaults to for that language (and its serif
counterpart), written under the **native** font names Excel stores — `游ゴシック`,
`ＭＳ 明朝` with its full-width letters, `等线`, `新細明體`, `맑은 고딕` — because those
are what a real file carries and what the name mapping has to recognise.

Plus the two fallback cases: a Calibri cell holding Japanese and one holding
Korean, which Excel draws through font linking, and a bold `ＭＳ Ｐゴシック`
cell with Latin in it.

**Built here, not by Office**: no Excel-made file with these fonts has cleared
the corpus rights review (fixtures/README.md). The XML is what Excel writes for
a font element (`<name val>`, `<charset>`, `<family>`), so the importer sees
the same shape.

Regenerate with:

    python3 fixtures/tools/cjk.py
"""

import pathlib
import zipfile
from xml.sax.saxutils import escape

OUT = pathlib.Path("fixtures/generated/cjk-east-asian.xlsx")
FIXED_DATE = (1980, 1, 1, 0, 0, 0)

# (text, font name, charset, family, bold). Charset values are the GDI ones
# Excel writes: 128 SHIFTJIS, 134 GB2312, 136 CHINESEBIG5, 129 HANGUL.
ROWS = [
    ("日本語のテキスト", "游ゴシック", 128, 3, False),
    ("明朝体で書かれた文章", "ＭＳ 明朝", 128, 1, False),
    ("简体中文文本", "等线", 134, 2, False),
    ("宋体的中文", "宋体", 134, 2, False),
    ("繁體中文文字", "微軟正黑體", 136, 2, False),
    ("新細明體的字", "新細明體", 136, 1, False),
    ("한국어 텍스트", "맑은 고딕", 129, 3, False),
    ("바탕체 글자", "바탕", 129, 1, False),
    ("Calibri と日本語", "Calibri", None, 2, False),
    ("Calibri 한국어", "Calibri", None, 2, False),
    ("Latin ABC 太字", "ＭＳ Ｐゴシック", 128, 3, True),
    # MS Mincho has no bold; Excel draws it emboldened.
    ("太字の明朝", "ＭＳ 明朝", 128, 1, True),
]

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
    '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
    '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'
    "</Types>"
)
RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
    "</Relationships>"
)
WORKBOOK = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
    '<sheets><sheet name="CJK" sheetId="1" r:id="rId1"/></sheets></workbook>'
)
WORKBOOK_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>'
    "</Relationships>"
)


def font_xml(name, charset, family, bold):
    parts = ["<font>"]
    if bold:
        parts.append("<b/>")
    parts.append('<sz val="11"/><color theme="1"/>')
    parts.append(f'<name val="{escape(name)}"/><family val="{family}"/>')
    if charset is not None:
        parts.append(f'<charset val="{charset}"/>')
    parts.append("</font>")
    return "".join(parts)


def styles():
    fonts = [font_xml("Calibri", None, 2, False)] + [font_xml(n, c, f, b) for _, n, c, f, b in ROWS]
    xfs = ['<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'] + [
        f'<xf numFmtId="0" fontId="{i + 1}" fillId="0" borderId="0" xfId="0" applyFont="1"/>'
        for i in range(len(ROWS))
    ]
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        f'<fonts count="{len(fonts)}">{"".join(fonts)}</fonts>'
        '<fills count="2"><fill><patternFill patternType="none"/></fill>'
        '<fill><patternFill patternType="gray125"/></fill></fills>'
        '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
        '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
        f'<cellXfs count="{len(xfs)}">{"".join(xfs)}</cellXfs>'
        '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
        "</styleSheet>"
    )


def shared_strings():
    items = "".join(f"<si><t>{escape(t)}</t></si>" for t, *_ in ROWS)
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        f'count="{len(ROWS)}" uniqueCount="{len(ROWS)}">{items}</sst>'
    )


def sheet():
    rows = "".join(
        f'<row r="{i + 1}"><c r="A{i + 1}" s="{i + 1}" t="s"><v>{i}</v></c></row>'
        for i in range(len(ROWS))
    )
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        # Wide enough for every row, so a printout shows the whole text rather
        # than what fits before the print area's edge.
        '<cols><col min="1" max="1" width="36" customWidth="1"/></cols>'
        f"<sheetData>{rows}</sheetData></worksheet>"
    )


def main():
    parts = {
        "[Content_Types].xml": CONTENT_TYPES,
        "_rels/.rels": RELS,
        "xl/workbook.xml": WORKBOOK,
        "xl/_rels/workbook.xml.rels": WORKBOOK_RELS,
        "xl/styles.xml": styles(),
        "xl/sharedStrings.xml": shared_strings(),
        "xl/worksheets/sheet1.xml": sheet(),
    }
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as zf:
        for name in sorted(parts):
            info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, parts[name].encode("utf-8"))
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
