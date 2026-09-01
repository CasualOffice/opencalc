// The printout is a file a user can actually get (`IO-14`).
//
// `IO-03` and `IO-10` finished the paginator and the PDF writer — print area,
// print titles, headers and footers, the field-code language, repeated rows —
// and none of it reached anybody. `casual-calc-sdk` is `publish = false`, the
// server has no PDF route, and this editor compiled `export_pdf` into every
// build without ever offering it. Work that is done and unreachable is
// indistinguishable, from where a user stands, from work that was never done.
//
// So the assertion is about the bytes, not about the menu: a `%PDF-` header, a
// `%%EOF` trailer, and a page whose size responds to the sheet's own page
// setup. A test that only checked the command exists would pass against a
// binding that returned an empty vector.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const exportBytes = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "Revenue");
    a.session_set_cell(0, 1, 0, "1234.5");
    return Array.from(a.session_export_pdf(0));
  });

test("the engine hands the editor a real PDF", async ({ page }) => {
  await boot(page);
  const bytes = await exportBytes(page);

  expect(bytes.length, "an empty export is what an unwired binding returns").toBeGreaterThan(400);
  const head = String.fromCharCode(...bytes.slice(0, 5));
  expect(head, `the file does not begin as a PDF: ${JSON.stringify(head)}`).toBe("%PDF-");
  const tail = String.fromCharCode(...bytes.slice(-32));
  expect(tail, `no end-of-file marker: ${JSON.stringify(tail)}`).toContain("%%EOF");
});

/// **The page setup is honoured, not defaulted.** A writer that emitted one
/// hardcoded page size would pass every check above, so the size is changed and
/// the output has to follow — this is the difference between exporting the
/// sheet and exporting a blank A4.
test("the exported page follows the sheet's own paper size", async ({ page }) => {
  await boot(page);
  const [a4, letter] = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "x");
    const read = () => {
      const bytes = a.session_export_pdf(0);
      const text = new TextDecoder("latin1").decode(bytes);
      const m = text.match(/MediaBox\s*\[\s*0\s+0\s+([0-9.]+)\s+([0-9.]+)/);
      return m ? [Number(m[1]), Number(m[2])] : null;
    };
    // `page.paperSize` is OOXML's numbered stock enum: 9 is A4, 1 is Letter.
    a.session_set_page_setup(0, ["page.paperSize"], ["9"]);
    const one = read();
    a.session_set_page_setup(0, ["page.paperSize"], ["1"]);
    return [one, read()];
  });

  expect(a4, "no MediaBox in the output at all").not.toBeNull();
  expect(letter, "no MediaBox in the output at all").not.toBeNull();
  expect(
    a4,
    `A4 and Letter produced the same page box ${JSON.stringify(a4)} — the paper size is not reaching the writer`,
  ).not.toEqual(letter);
});
