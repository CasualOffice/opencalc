// A sheet named like a reference is written quoted (XL-UX-166).
//
// Excel writes a sheet name bare only when it is a plain identifier that
// cannot be read as anything else; a name that starts with a digit, holds a
// space or punctuation, or *is* a reference in either notation (`A1`, `R1C1`,
// `XFD1048576`, `R`, `C`) is written in single quotes — `'A1'!$B$2`. Written
// bare, `A1!B2` reads as a different reference, or as nothing.
//
// The webapp had five private copies of the rule, none of them the engine's:
// Go To's recent list, New Name's Refers to, the name box defining a name,
// picking a cell on another sheet while typing a formula, and a chart's
// series. Each left `A1` and `R1C1` bare. They now share the engine's own rule
// (`sheet_name_quoted`, the printer's `CellReference` display).

import { expect, test } from "@playwright/test";

async function boot(page, name) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate((name) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_rename_sheet(0, name);
    a.session_set_cell(0, 1, 1, "here");   // B2
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.draw();
  }, name);
  await page.locator("#grid").focus();
}

const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());

for (const name of ["A1", "R1C1", "XFD1048576", "R", "2024", "My Sheet"]) {
  test(`Go To remembers B2 on a sheet named ${name} so it reads back`, async ({ page }) => {
    await boot(page, name);
    await page.keyboard.press("Control+g");
    await page.locator("#oc-goto-ref").fill("B2");
    await page.keyboard.press("Enter");
    await expect(page.locator("#oc-modal")).toBeHidden();

    await page.evaluate(() => window.opencalcEditor.selectForTest(5, 5));
    await page.keyboard.press("Control+g");
    const recent = page.locator("#oc-goto-list [role=option]").first();
    await expect(recent).toHaveText(`'${name}'!$B$2`);
    // The round trip: the remembered text goes back to where it came from.
    await recent.click();
    await page.locator("#oc-modal .oc-btn.primary").click();
    await expect(page.locator("#oc-msgbox")).toBeHidden();
    expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
  });
}

test("a plain sheet name is still written bare", async ({ page }) => {
  await boot(page, "Data");
  await page.keyboard.press("Control+g");
  await page.locator("#oc-goto-ref").fill("B2");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Control+g");
  await expect(page.locator("#oc-goto-list [role=option]").first()).toHaveText("Data!$B$2");
});

test("New Name's Refers to quotes a sheet named R1C1", async ({ page }) => {
  await boot(page, "R1C1");
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(1, 1);
    window.opencalcEditor.runCommand("formulas.define-name");
  });
  await expect(page.locator("#oc-name-ref")).toHaveValue("='R1C1'!$B$2");
});

test("the name box defines a name on a sheet named A1 that resolves", async ({ page }) => {
  await boot(page, "A1");
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.fill("#cell-ref", "Spot");
  await page.press("#cell-ref", "Enter");
  const names = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_names()));
  expect(names.map((n) => n.name)).toContain("Spot");
  expect(names.find((n) => n.name === "Spot").refersTo).toMatch(/^'A1'!/);
});

test("a chart on a sheet named A1 draws its series from that sheet", async ({ page }) => {
  await boot(page, "A1");
  const refs = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "k"); a.session_set_cell(0, 0, 1, "v");
    a.session_set_cell(0, 1, 0, "x"); a.session_set_cell(0, 1, 1, "1");
    a.session_set_cell(0, 2, 0, "y"); a.session_set_cell(0, 2, 1, "2");
    a.session_create_chart(0, 0, 0, 2, 1, "bar");
    return a.session_chart_defs(0);
  });
  expect(refs).toContain("'A1'!$B$2:$B$3");
});
