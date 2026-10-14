// Hidden defined names stay out of Excel's lists (fit-gap XL-FEAT-124).
//
// `<definedName hidden="1">` is add-in and macro plumbing — `_xlnm._FilterDatabase`
// and the like. The importer dropped the attribute, so such names were listed
// in the Name Manager, the name box and Go To, and written back visible on the
// next save. The engine now keeps the flag and reports it from
// `session_names`; the editor's three lists leave hidden names out, as
// Excel's do. The name is still defined — a formula using it still resolves.

import { expect, test } from "@playwright/test";

/// A minimal `.xlsx` with one hidden and one visible workbook name, written as
/// a *stored* (uncompressed) zip so the test needs no library: the bytes are
/// what the file says, with nothing between the assertion and the input.
function hiddenNamesPackage() {
  const parts = {
    "[Content_Types].xml":
      '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
      '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
      '<Default Extension="xml" ContentType="application/xml"/>' +
      '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>' +
      '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' +
      "</Types>",
    "_rels/.rels":
      '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
      '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>' +
      "</Relationships>",
    "xl/workbook.xml":
      '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">' +
      '<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>' +
      "<definedNames>" +
      '<definedName name="Plumbing" hidden="1" function="1">Sheet1!$A$1</definedName>' +
      '<definedName name="Visible">Sheet1!$A$1</definedName>' +
      "</definedNames></workbook>",
    "xl/_rels/workbook.xml.rels":
      '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
      '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>' +
      "</Relationships>",
    "xl/worksheets/sheet1.xml":
      '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">' +
      '<sheetData><row r="1"><c r="A1"><v>42</v></c></row></sheetData></worksheet>',
  };
  const enc = new TextEncoder();
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc32 = (b) => {
    let c = 0xffffffff;
    for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const u16 = (v) => [v & 0xff, (v >>> 8) & 0xff];
  const u32 = (v) => [v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff];
  const local = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of Object.entries(parts)) {
    const n = enc.encode(name);
    const d = enc.encode(text);
    const crc = crc32(d);
    const common = [...u16(20), ...u16(0), ...u16(0), ...u16(0), ...u16(0), ...u32(crc), ...u32(d.length), ...u32(d.length), ...u16(n.length), ...u16(0)];
    const head = [...u32(0x04034b50), ...common, ...n];
    central.push(...u32(0x02014b50), ...u16(20), ...common, ...u16(0), ...u16(0), ...u16(0), ...u32(0), ...u32(offset), ...n);
    local.push(...head, ...d);
    offset += head.length + d.length;
  }
  const count = Object.keys(parts).length;
  const end = [...u32(0x06054b50), ...u16(0), ...u16(0), ...u16(count), ...u16(count), ...u32(central.length), ...u32(offset), ...u16(0)];
  return [...local, ...central, ...end];
}

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  const opened = await page.evaluate(
    (bytes) => window.opencalcEditor.openBytes(new Uint8Array(bytes), "hidden-names.xlsx"),
    hiddenNamesPackage(),
  );
  expect(opened, "the package opened").toBeTruthy();
  await page.locator("#grid").focus();
}

test("the engine reports a hidden name as hidden, and it still resolves", async ({ page }) => {
  await boot(page);
  const names = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_names()));
  const by = Object.fromEntries(names.map((n) => [n.name, n.hidden]));
  expect(by).toEqual({ Plumbing: true, Visible: false });
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_evaluate(0, "Plumbing*2"))).toBe("84");
});

test("the Name Manager does not list a hidden name", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+F3");
  await expect(page.locator("#oc-modal-title")).toHaveText("Name Manager");
  await expect(page.locator('#oc-nm-list .oc-nm-row [data-col="name"]')).toHaveText(["Visible"]);
});

test("Go To does not offer a hidden name", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  const list = page.locator("#oc-goto-list");
  await expect(list).toContainText("Visible");
  await expect(list).not.toContainText("Plumbing");
});

test("the name box's list does not offer a hidden name", async ({ page }) => {
  await boot(page);
  await page.locator("#name-box-list").click();
  const menu = page.locator("#sheet-ctx");
  await expect(menu.locator(".menu-item")).toHaveText(["Visible"]);
});
