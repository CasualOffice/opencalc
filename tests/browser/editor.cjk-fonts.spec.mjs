// Japanese, Chinese and Korean render, measure and export with the faces this
// project ships (`XL-FEAT-140`).
//
// Before: the bundled faces were Latin-only, so a cell styled 游ゴシック or
// 宋体 drew whatever the viewer happened to have installed (nothing, on a
// machine without CJK fonts), autofit measured with that stand-in, and the PDF
// drew no CJK at all.
//
// The fixture is `fixtures/generated/cjk-east-asian.xlsx`: one row per region
// and style in the font Excel defaults to for that language, plus Calibri cells
// holding Japanese and Korean (Excel's font linking) and bold MS Mincho.

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const FIXTURE = new URL("../../fixtures/generated/cjk-east-asian.xlsx", import.meta.url);

/// Every face the fixture's text needs: four regions, sans and serif.
const FAMILIES = [
  "OpenCalc CJK JP Sans",
  "OpenCalc CJK JP Serif",
  "OpenCalc CJK SC Sans",
  "OpenCalc CJK SC Serif",
  "OpenCalc CJK TC Sans",
  "OpenCalc CJK TC Serif",
  "OpenCalc CJK KR Sans",
  "OpenCalc CJK KR Serif",
];

async function boot(page) {
  // `?cjk=jp`: the default region is the viewer's language otherwise, and a
  // gate must not depend on the machine it runs on.
  await page.goto("/editor.html?cjk=jp");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

async function openFixture(page) {
  await page.evaluate(async (bytes) => {
    window.opencalcEditor.wasmApi().session_open(new Uint8Array(bytes));
    window.opencalcEditor.relayout();
  }, [...readFileSync(FIXTURE)]);
}

const loadedFamilies = (page) =>
  page.evaluate(() =>
    [...new Set([...document.fonts]
      .filter((f) => f.family.replace(/"/g, "").startsWith("OpenCalc CJK") && f.status === "loaded")
      .map((f) => f.family.replace(/"/g, "")))].sort(),
  );

test("each region's text is drawn with that region's shipped face, fetched slice by slice", async ({ page }) => {
  const slices = [];
  page.on("request", (r) => { if (/\/fonts\/cjk\/.+\.woff2/.test(r.url())) slices.push(r.url()); });
  await boot(page);
  await openFixture(page);

  await expect
    .poll(() => loadedFamilies(page), { timeout: 30_000, message: "the CJK faces the fixture needs did not load" })
    .toEqual([...FAMILIES].sort());

  // Ink where the text is: the grid was redrawn once the faces arrived.
  const ink = await page.evaluate(() => {
    const c = document.querySelector("canvas");
    const g = c.getContext("2d");
    const { data } = g.getImageData(0, 0, c.width, c.height);
    let dark = 0;
    for (let i = 0; i < data.length; i += 4) if (data[i] < 90 && data[i + 1] < 90 && data[i + 2] < 90) dark += 1;
    return dark;
  });
  expect(ink, "no glyph pixels on the grid").toBeGreaterThan(500);

  // On demand: a dozen short cells fetch a few dozen slices, not the 1,600.
  test.info().annotations.push({ type: "slices fetched for the fixture", description: String(slices.length) });
  console.log(`[XL-FEAT-140] slices fetched to draw the fixture: ${slices.length}`);
  expect(slices.length).toBeGreaterThan(8);
  expect(slices.length).toBeLessThan(80);
});

test("autofit measures CJK with the face that draws it, and re-fits when that face arrives", async ({ page }) => {
  await boot(page);
  // 游ゴシック with Latin and kana: Excel draws both from the East Asian face,
  // whose Latin is narrower than the Latin stand-in a missing mapping falls
  // back to — so the width only comes out right if the right face measured it.
  const text = "Total 合計 ABC あいう";
  const pendingAtOnce = await page.evaluate((t) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, t);
    a.session_set_font_name(0, 0, 0, 0, 0, "游ゴシック");
    a.session_clear_col_width(0, 0);
    // Straight away, before anything has drawn the cell: its slices are not
    // loaded, so this first fit measures a stand-in and must be redone. Read
    // in the same task — from a local server the faces can arrive before a
    // second `evaluate` would look.
    window.opencalcEditor.autofitColumnForTest(0);
    return window.opencalcEditor.autofitPendingForTest();
  }, text);
  expect(pendingAtOnce, "the first fit did not notice its face was still loading").toBe(1);

  await expect.poll(() => page.evaluate(() => window.opencalcEditor.autofitPendingForTest()), { timeout: 30_000 }).toBe(0);
  const [width, expected] = await page.evaluate(async (t) => {
    const a = window.opencalcEditor.wasmApi();
    // What the text measures in the JP sans face alone, now that it is loaded.
    // At the size the editor draws the cell at (its own px rounding).
    const cell = JSON.parse(a.session_cells(0, 0, 0, 0, 0))[0];
    const px = window.opencalcEditor.cellFont(cell).match(/([\d.]+)px/)[1];
    const g = document.createElement("canvas").getContext("2d");
    await document.fonts.load(`${px}px "OpenCalc CJK JP Sans"`, t);
    g.font = `${px}px "OpenCalc CJK JP Sans"`;
    return [a.session_col_width(0, 0), Math.ceil(g.measureText(t).width) + 14];
  }, text);
  expect(Math.abs(width - expected), `autofit ${width}px, the text in its face needs ${expected}px`).toBeLessThanOrEqual(1);
});

test("the PDF export fetches the slices it needs and names nothing as undrawn", async ({ page }) => {
  await boot(page);
  await openFixture(page);
  const before = await page.evaluate(() => window.opencalcEditor.wasmApi().session_export_pdf_loss(0));
  expect(before, "without its fonts the export must say what it could not draw").toMatch(/CJK/);

  const fetched = await page.evaluate(() => window.opencalcEditor.ensureExportFonts(0));
  test.info().annotations.push({ type: "slices fetched for the PDF", description: String(fetched) });
  console.log(`[XL-FEAT-140] slices fetched to export the fixture: ${fetched}`);
  expect(fetched).toBeGreaterThan(8);

  const [after, bytes] = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [a.session_export_pdf_loss(0), a.session_export_pdf(0).length];
  });
  expect(after, "every character drawn").not.toMatch(/no installed face/);
  expect(bytes).toBeGreaterThan(20_000);
});
