#!/usr/bin/env python3
"""Build a probe workbook in which every audited element carries *every*
attribute the schema declares for it, then report what survives a round trip.

Element *placement* is hand-specified (small, reviewable) because valid OOXML
needs correct nesting; the *attribute set* on each element comes from the schema,
so no attribute can be forgotten. Values are deliberately non-default: writing a
default back is indistinguishable from preserving it, and would score a dropped
attribute as kept.
"""
import pathlib, sys, zipfile
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from schema import Schema

S = Schema()

# Non-default sample values. Keyed by attribute name first (names are far more
# informative than the generic ST_ types), then by type, then by enum facet.
BY_NAME = {
    "min": "2", "max": "4", "width": "23.5", "r": "1", "spans": "1:6",
    "ref": "A1:B2", "sqref": "A1:B2", "topLeftCell": "B2", "id": "1",
    "sheetId": "1", "name": "Probe", "displayName": "Probe",
    "count": "1", "uniqueCount": "1", "workbookViewId": "0",
}
def sample(attr, typ, elem, default=None):
    if elem == "c" and attr == "r": return "A1"
    if attr in BY_NAME: return BY_NAME[attr]
    enums = S.enum_values(typ)
    if enums:
        # Never the declared default, and never the "absent" spellings: an
        # omitted default is correct behaviour, not a loss.
        skip = {default, "none", "general", "default", "normal", "auto"}
        for v in enums:
            if v not in skip: return v
        return enums[0]
    if typ in ("ST_Xstring", "s:ST_Xstring", "xsd:string", "ST_CellRef", "ST_Ref", "ST_Sqref"):
        return "A1"
    if typ in ("xsd:boolean", "ST_Boolean"):
        return "0" if str(default).lower() in ("true", "1") else "1"
    if typ in ("xsd:double", "xsd:float"): return "12.5"
    return "1"

def tag(elem, extra_skip=(), self_close=True, inner=""):
    attrs = [(n, t, d) for n, t, d in S.attrs_of_element(elem) if n not in extra_skip]
    body = "".join(f' {n}="{sample(n, t, elem, d)}"' for n, t, d in attrs)
    if self_close and not inner: return f"<{elem}{body}/>", [n for n, _, _ in attrs]
    return f"<{elem}{body}>{inner}</{elem}>", [n for n, _, _ in attrs]

expect = {}
def E(elem, **kw):
    xml, names = tag(elem, **kw)
    expect[elem] = names
    return xml

NS = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'

sheet = f'''<?xml version="1.0"?><worksheet {NS}>
<sheetPr><outlinePr summaryBelow="0" summaryRight="0"/><tabColor rgb="FF00B0F0"/></sheetPr>
<sheetViews><sheetView workbookViewId="0" showGridLines="0" showRowColHeaders="0" zoomScale="80" tabSelected="1" rightToLeft="1" showZeros="0" showFormulas="1">
<pane xSplit="1" ySplit="2" topLeftCell="B3" activePane="bottomRight" state="frozen"/>
</sheetView></sheetViews>
<sheetFormatPr defaultColWidth="11.5" defaultRowHeight="16.5" customHeight="1" zeroHeight="1" thickTop="1" thickBottom="1" outlineLevelRow="1" outlineLevelCol="1"/>
<cols>{E("col")}</cols>
<sheetData>
<row r="1" spans="1:3" ht="22.5" customHeight="1" hidden="1" outlineLevel="1" collapsed="1" s="1" customFormat="1" thickTop="1" thickBot="1">
<c r="A1" s="1" t="str"><f>1+1</f><v>2</v></c></row>
</sheetData>
<mergeCells count="1"><mergeCell ref="B1:C1"/></mergeCells>
<dataValidations count="1">{E("dataValidation", extra_skip=("sqref",), self_close=False, inner="<formula1>1</formula1><formula2>10</formula2>").replace("<dataValidation", '<dataValidation sqref="A1"')}</dataValidations>
<hyperlinks>{E("hyperlink")}</hyperlinks>
<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>
</worksheet>'''

CT = '''<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>'''
ROOTRELS = '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>'
WB = f'''<?xml version="1.0"?><workbook {NS}><sheets><sheet name="Probe" sheetId="1" r:id="rId1" state="hidden"/></sheets>
<definedNames><definedName name="Rng" localSheetId="0" hidden="1">Probe!$A$1</definedName></definedNames>
<workbookPr date1904="1"/></workbook>'''
WBRELS = '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>'
SHEETRELS = '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/></Relationships>'
STYLES = f'''<?xml version="1.0"?><styleSheet {NS}>
<fonts count="1"><font><b/><i/><u val="double"/><strike/><sz val="13"/><color rgb="FF112233"/><name val="Verdana"/><family val="2"/><scheme val="minor"/><charset val="1"/><vertAlign val="superscript"/></font></fonts>
<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFEEDDCC"/><bgColor rgb="FF001122"/></patternFill></fill></fills>
<borders count="1"><border diagonalUp="1" diagonalDown="1" outline="0"><left style="thin"><color rgb="FF111111"/></left><right style="medium"/><top style="dashed"/><bottom style="double"/><diagonal style="hair"/><horizontal style="hair"/><vertical style="hair"/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0"/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0"/>
<xf numFmtId="0" fontId="0" fillId="1" borderId="0" xfId="0" quotePrefix="1" pivotButton="1" applyNumberFormat="1" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1" applyProtection="1">
<alignment horizontal="centerContinuous" vertical="justify" textRotation="45" wrapText="1" indent="2" relativeIndent="1" justifyLastLine="1" shrinkToFit="1" readingOrder="2"/>
<protection locked="0" hidden="1"/></xf></cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>'''

out = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "probe.xlsx")
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", CT); z.writestr("_rels/.rels", ROOTRELS)
    z.writestr("xl/workbook.xml", WB); z.writestr("xl/_rels/workbook.xml.rels", WBRELS)
    z.writestr("xl/styles.xml", STYLES); z.writestr("xl/worksheets/sheet1.xml", sheet)
    z.writestr("xl/worksheets/_rels/sheet1.xml.rels", SHEETRELS)
print(f"wrote {out}")
