// Formulas answer the way Excel does where binary floating point and text
// coercion used to make them differ, typed through the editor as a user would.
//
// XL-FEAT-011: `=0.1+0.2=0.3` is TRUE in Excel, which compares to 15
// significant digits; it was FALSE here, and every IF over computed money was
// one ulp from the wrong branch.
// XL-FEAT-019: `=VALUE("1,234")` and `=DATEVALUE("12/31/2026")` were #VALUE!
// while typing the same text into a cell read it fine. Formulas and typing now
// share one reader.

import { expect, test } from "@playwright/test";

async function typeInto(page, ref, text) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}

/// What column A shows, row by row, as display text.
const shown = (page, rows) =>
  page.evaluate((rows) => {
    const cells = JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, 0, 0, rows - 1, 0));
    const out = Array(rows).fill("");
    for (const c of cells) if (c.c === 0) out[c.r] = c.t;
    return out;
  }, rows);

test("comparisons and text coercion answer as Excel does", async ({ page }) => {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  const formulas = [
    ["=0.1+0.2=0.3", "TRUE"],
    ["=1=1+1E-10", "FALSE"],
    ['=IF(0.1+0.2=0.3,"same","differ")', "same"],
    ["=1.333+1.225-1.333-1.225", "0"],
    ['=VALUE("1,234")', "1234"],
    ['=DATEVALUE("12/31/2026")', "46387"],
    ['="$5.00"*2', "10"],
    ['=TIMEVALUE("12:00 PM")', "0.5"],
  ];
  for (const [i, [formula]] of formulas.entries()) await typeInto(page, `A${i + 1}`, formula);
  const got = await shown(page, formulas.length);
  for (const [i, [formula, want]] of formulas.entries()) {
    expect(got[i], `${formula}`).toBe(want);
  }
});
