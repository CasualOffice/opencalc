// Every format the engine can write is reachable, and each one says what it costs.
//
// `IO-07` gave the engine ODS bytes and `IO-08` gave it macro-enabled `.xlsm`,
// and for a while the editor could produce neither: `saveAs` knew four format
// names and the Download submenu listed five entries by hand. That is the
// engine-capable / editor-cannot shape twice over, and it comes with a second
// defect that is worse than the missing feature — `File ▸ Download ▸ Excel`
// on a macro workbook dropped the VBA project and said nothing, because the
// only loss report the editor asked for was about the format the document was
// *opened* from.
//
// So there are two claims here and they are separate:
//   1. the list of formats is the engine's answer, not a list kept in the page
//   2. the warning is about the format the person picked

import { expect, test } from "@playwright/test";

// The smallest macro-enabled package the engine will read: one sheet, `A1=41`,
// and an `xl/vbaProject.bin` hung off `xl/workbook.xml` by a
// `…/office/2006/relationships/vbaProject` relationship — which is what
// `Workbook::macro_project()` looks for. The `.bin` is not a real VBA project
// and never needs to be: the engine keeps it verbatim and never parses it.
//
// A base64 blob rather than a fixture file because it belongs to this test and
// nothing else. To regenerate: a stored/deflated zip of `[Content_Types].xml`
// (declaring `/xl/workbook.xml` as
// `application/vnd.ms-excel.sheet.macroEnabled.main+xml` and `bin` as
// `application/vnd.ms-office.vbaProject`), `_rels/.rels`, `xl/workbook.xml`,
// `xl/_rels/workbook.xml.rels` (carrying the vbaProject relationship),
// `xl/worksheets/sheet1.xml` and `xl/vbaProject.bin`.
const MACRO_WORKBOOK = [
  "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",
].join("");

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/** Open the macro workbook, and optionally give it a second sheet. */
async function openMacroWorkbook(page, extraSheet = false) {
  const ok = await page.evaluate(
    ([b64, extraSheet]) => {
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i += 1) bytes[i] = bin.charCodeAt(i);
      const opened = window.opencalcEditor.openBytes(bytes, "macros.xlsm");
      if (opened && extraSheet) window.opencalcEditor.wasmApi().session_add_sheet();
      return opened;
    },
    [MACRO_WORKBOOK, extraSheet],
  );
  expect(ok, "the fixture is a package this build reads").toBe(true);
  await expect
    .poll(() => page.evaluate(() => window.opencalcEditor.wasmApi().session_format()))
    .toBe("xlsm");
}

/**
 * Run a save and hand back the bytes it produced.
 *
 * Captured rather than performed, the way `editor.formats.spec.mjs` does it: a
 * real download is a dialog nothing can assert on, and what matters here is the
 * bytes and the type the page put on them. The loss dialog is answered with its
 * primary button, because these cases are about what gets written *after* the
 * user has said yes.
 */
async function saveAndCapture(page, fmt) {
  return await page.evaluate(async (fmt) => {
    const realCreate = URL.createObjectURL;
    const realRevoke = URL.revokeObjectURL;
    const realClick = HTMLAnchorElement.prototype.click;
    const blobs = [];
    const names = [];
    URL.createObjectURL = (blob) => { blobs.push(blob); return "blob:captured"; };
    URL.revokeObjectURL = () => {};
    HTMLAnchorElement.prototype.click = function () { names.push(this.download); };
    try {
      const running = window.opencalcEditor.saveAs(fmt, "download");
      for (let i = 0; i < 60 && !names.length; i += 1) {
        await new Promise((r) => setTimeout(r, 25));
        const modal = document.getElementById("oc-modal");
        if (modal && !modal.hidden) document.querySelector("#oc-modal-body .oc-btn.primary")?.click();
      }
      await running;
      if (!blobs.length) return { name: null, type: null, head: null, size: 0 };
      const bytes = new Uint8Array(await blobs[0].arrayBuffer());
      // The first 120 bytes as latin-1, which is where a package's magic and
      // ODS's `mimetype` entry both live.
      let head = "";
      for (let i = 0; i < Math.min(bytes.length, 120); i += 1) head += String.fromCharCode(bytes[i]);
      return { name: names[0] ?? null, type: blobs[0].type, head, size: bytes.length };
    } finally {
      URL.createObjectURL = realCreate;
      URL.revokeObjectURL = realRevoke;
      HTMLAnchorElement.prototype.click = realClick;
    }
  }, fmt);
}

/** Start a save, read the loss dialog, and cancel it. */
async function lossDialogFor(page, fmt) {
  await page.evaluate((fmt) => { window.opencalcEditor.saveAs(fmt, "download"); }, fmt);
  await expect(page.locator("#oc-modal")).toBeVisible();
  const text = await page.locator("#oc-modal-body .oc-confirm-text").textContent();
  const title = await page.locator("#oc-modal-title").textContent();
  await page.locator("#oc-modal-body .oc-btn", { hasText: "Cancel" }).click();
  await expect(page.locator("#oc-modal")).toBeHidden();
  return { title, text };
}

test("saving as .ods produces OpenDocument bytes, not a workbook under an .ods name", async ({ page }) => {
  // The magic, not the file name. A save that wrote an OOXML package and called
  // it `.ods` would pass every name-shaped assertion and fail every user.
  await boot(page);
  const out = await saveAndCapture(page, "ods");
  expect(out.name).toBe("opencalc.ods");
  expect(out.head.slice(0, 2), "a zip container").toBe("PK");
  expect(
    out.head,
    "ODS declares itself in an uncompressed `mimetype` entry first in the package",
  ).toContain("mimetypeapplication/vnd.oasis.opendocument.spreadsheet");
  expect(out.type).toBe("application/vnd.oasis.opendocument.spreadsheet");
});

test("saving as .xlsm produces a macro-enabled package with its own type", async ({ page }) => {
  await boot(page);
  const out = await saveAndCapture(page, "xlsm");
  expect(out.name).toBe("opencalc.xlsm");
  expect(out.head.slice(0, 2)).toBe("PK");
  // Lower-cased because that is what `new Blob()` does to a type; the engine's
  // own answer keeps the capital E.
  expect(out.type).toBe("application/vnd.ms-excel.sheet.macroenabled.12");
  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().format_content_type("xlsm")),
  ).toBe("application/vnd.ms-excel.sheet.macroEnabled.12");
});

test("the loss warning is about the format the user picked, not the one they opened", async ({ page }) => {
  // The same workbook, two formats, two different answers. `session_save_loss()`
  // could not produce this: it only ever described the session's own format, so
  // both of these were the same sentence — and for an `.xlsm` that sentence was
  // empty, because writing an `.xlsm` back loses nothing.
  await boot(page);
  await openMacroWorkbook(page, true);

  const csv = await lossDialogFor(page, "csv");
  const ods = await lossDialogFor(page, "ods");

  expect(csv.text).not.toBe(ods.text);
  expect(csv.text, "a .csv holds one sheet").toMatch(/other sheets/);
  expect(ods.text, "an .ods holds every sheet").not.toMatch(/other sheets/);
  // Both know about the macros, because that is a fact about the target format.
  expect(csv.text).toMatch(/macros \(VBA project\)/);
  expect(ods.text).toMatch(/macros \(VBA project\)/);
});

test("converting a macro workbook to .xlsx says the macros are going", async ({ page }) => {
  // `IO-08`: an OOXML package holding a VBA project while declaring itself a
  // plain workbook is one Excel opens as damaged and repairs by deleting the
  // project — so the engine drops it on the way out, correctly, and the only
  // remaining question is whether anybody is told. This is that question.
  await boot(page);
  await openMacroWorkbook(page);

  const xlsx = await lossDialogFor(page, "xlsx");
  expect(xlsx.title).toMatch(/\.xlsx cannot carry all of this/);
  expect(xlsx.text).toMatch(/macros \(VBA project\)/);

  // And saving it back as `.xlsm` asks nothing, because nothing is lost.
  const out = await saveAndCapture(page, "xlsm");
  expect(out.name).toBe("opencalc.xlsm");
  expect(await page.locator("#oc-modal").isHidden()).toBe(true);
});

test("the Download list is the engine's answer, not one the page keeps", async ({ page }) => {
  await boot(page);
  const seen = await page.evaluate(async () => {
    const sheets = await import("/editor.sheets.js");
    const w = window.opencalcEditor.wasmApi();
    return {
      writable: sheets.writableFormats(),
      labels: sheets.downloadItems().map((item) => item[0]),
      engine: JSON.parse(w.writable_extensions()).map((x) => String(x).replace(/^[."\']+|["\']+$/g, "")),
      openable: JSON.parse(w.openable_extensions()).map((x) => String(x).replace(/^[."\']+|["\']+$/g, "")),
    };
  });

  expect(seen.writable, "asked, not remembered").toEqual(seen.engine);
  expect(seen.writable).toContain("ods");
  expect(seen.writable).toContain("xlsm");
  // Narrower than what the engine *reads*, and deliberately: `.tab` names the
  // TAB delimiter, whose own extension is `tsv`. Offering it would be a menu
  // entry whose save then refuses.
  expect(seen.openable).toContain("tab");
  expect(seen.writable, "a format that is read is not necessarily written").not.toContain("tab");

  // "Same format as opened" leads, and every other entry is one of the engine's
  // formats — including the two that had no entry at all before this.
  expect(seen.labels[0]).toBe("Same format as opened");
  expect(seen.labels).toContain("OpenDocument (.ods)");
  expect(seen.labels).toContain("Excel macro-enabled (.xlsm)");
  expect(seen.labels.length).toBe(seen.writable.length + 1);
});
