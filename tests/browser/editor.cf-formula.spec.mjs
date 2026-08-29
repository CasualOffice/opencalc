// The conditional format that reaches a *different* cell from the one it paints.
//
// `CF-01` built the formula rule end to end in the engine — `CfRule::Expression`,
// import, export, and evaluation at both renderers — and the dialog offered no
// way to write one. Whole-row highlighting, the commonest conditional format in
// real workbooks, was reachable by the engine and not by a user: the eighth time
// in this repository that the editor could not reach something the engine had
// (`docs/12` §6 counts the category).
//
// What is asserted here is the part a user gets wrong and a screenshot cannot
// show: the formula is written **for the top-left cell of the range** and
// shifted for every other cell in it, which is the whole difference between
// `=$D2>100` (the row) and `=D2>100` (one column). So the range deliberately
// does not start at `A1` — a rule anchored at `A1` by accident would paint the
// same cells here for the wrong reason.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// A fresh workbook — the sample one applies its own cell styles, and a probe
/// written over it would be measuring the sample — holding a small table in
/// `B2:E6` with the amounts in column D, and that range selected.
///
/// Every cell of the table has a value on purpose. A conditional fill is
/// reported only for cells that exist in the sheet's store, so a *blank* cell
/// inside a matching row is not painted at all — see the note at the end of
/// this file. Filling the table keeps this test about the panel rather than
/// about that.
async function seed(page) {
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const w = ed.wasmApi();
    w.session_new();
    // A row per region, amounts in D: 150, 50, 900, 10, 200.
    const rows = [
      ["north", "q1", "150", "ok"],
      ["south", "q1", "50", "ok"],
      ["east", "q1", "900", "ok"],
      ["west", "q1", "10", "ok"],
      ["north", "q2", "200", "ok"],
    ];
    rows.forEach((cells, i) =>
      cells.forEach((v, j) => w.session_set_cell(0, 1 + i, 1 + j, v)));
    // Outside the range, and filled so that "not painted" is a real answer
    // rather than a cell the payload never mentions: A2 to the left of it, and
    // row 7 below it with an amount that would match.
    w.session_set_cell(0, 1, 0, "left");
    w.session_set_cell(0, 6, 1, "below");
    w.session_set_cell(0, 6, 3, "999");
    ed.selectForTest(1, 1);             // B2
    ed.extendSelectionForTest(5, 4);    // …E6
    ed.draw();
  });
}

const panelBody = (page) => page.locator("#side-panel-body");
const kindSelect = (page) => page.locator("#side-panel-body select.panel-select").first();
const formulaField = (page) => page.locator("#side-panel-body .cf-formula");
const problem = (page) => page.locator("#side-panel-body .panel-error");
const applyButton = (page) => page.locator("#side-panel-body button", { hasText: /^Apply$/ });

/// Through the command the Format menu dispatches, which is the route a user
/// takes: at this viewport the toolbar button itself has collapsed into the
/// overflow flyout, and clicking a collapsed button is not what a user does.
async function openCfPanel(page) {
  await page.evaluate(() => window.opencalcEditor.runCommand("format.conditional-formatting"));
  await expect(panelBody(page)).toBeVisible();
  await expect(page.locator("#side-panel-title")).toHaveText("Conditional formatting");
}

/// The background `session_cells` reports, keyed `"row,col"`. Only the painted
/// cells appear, so an absent key is a cell with no fill.
const fills = (page) =>
  page.evaluate(() => {
    const cells = JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, 0, 0, 12, 12));
    const at = {};
    for (const c of cells) if (c.bg) at[`${c.r},${c.c}`] = c.bg;
    return at;
  });

const cfRules = (page) =>
  page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_cf_rules(0)));

/// **The acceptance case: a whole row, not a single column.**
test("a custom formula authored in the panel paints whole rows", async ({ page }) => {
  await boot(page);
  await seed(page);
  await openCfPanel(page);

  await kindSelect(page).selectOption("formula");
  await formulaField(page).fill("=$D2>100");
  // Nothing is being rewritten: there is no rule here yet, and the panel says
  // so by staying quiet.
  await expect(panelBody(page).locator(".cf-editing")).toBeHidden();
  await applyButton(page).click();

  await expect
    .poll(async () => (await cfRules(page)).map((r) => `${r.range} ${r.desc}`))
    .toEqual(["B2:E6 formula =$D2>100"]);

  const bg = await fills(page);
  // Rows 2, 4 and 6 hold 150, 900 and 200, so every cell of those rows *inside
  // the range* is painted — B and E as much as D. A rule that painted only its
  // own column would pass a test that looked at D alone, and that is precisely
  // the mistake `=D2>100` makes.
  for (const row of [1, 3, 5]) {
    for (const col of [1, 2, 3, 4]) {
      expect(bg[`${row},${col}`], `row ${row + 1} column ${col} should be painted`).toBe("FFD166");
    }
  }
  // Rows 3 and 5 hold 50 and 10. Nothing in them is painted — which is what
  // proves the formula shifted with the cell rather than being evaluated once
  // at the anchor.
  for (const row of [2, 4]) {
    for (const col of [1, 2, 3, 4]) {
      expect(bg[`${row},${col}`], `row ${row + 1} column ${col} should be untouched`).toBeUndefined();
    }
  }
  // And nothing outside the range: A2 is one column to its left, and row 7
  // holds 999 but is one row below it.
  expect(bg["1,0"], "A2 is outside the range").toBeUndefined();
  expect(bg["6,1"], "row 7 is below the range").toBeUndefined();
  expect(bg["6,3"], "row 7 is below the range").toBeUndefined();
});

/// The anchor is stated, not left to be worked out.
test("the panel names the cell the formula is written for", async ({ page }) => {
  await boot(page);
  await seed(page);
  await openCfPanel(page);
  await kindSelect(page).selectOption("formula");

  const anchor = panelBody(page).locator(".cf-anchor");
  await expect(anchor).toBeVisible();
  // `B2`, not `A1`: the anchor is the range's top-left, and a panel that said
  // `A1` would be teaching the user the wrong rule.
  await expect(anchor.locator(".cf-anchor-cell")).toHaveText("B2");
  await expect(anchor).toContainText("B2:E6");

  // It follows the selection, because the panel stays open while cells are
  // picked — a stale anchor is worse than none.
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(9, 2);   // C10
    ed.extendSelectionForTest(12, 5);
    ed.draw();
  });
  await expect(anchor.locator(".cf-anchor-cell")).toHaveText("C10");
  await expect(anchor).toContainText("C10:F13");
});

/// A refusal has to land where the formula was typed.
test("a formula that cannot parse is refused, visibly, beside the field", async ({ page }) => {
  await boot(page);
  await seed(page);
  await openCfPanel(page);

  await kindSelect(page).selectOption("formula");
  await formulaField(page).fill("=$D2>");
  await applyButton(page).click();

  const message = problem(page);
  await expect(message).toBeVisible();
  await expect(message).toHaveText(/does not parse/);
  // Beside the field, not merely somewhere on the page: `docs/82` found a
  // dialog whose validation message appeared nowhere useful.
  const fieldBox = await formulaField(page).boundingBox();
  const messageBox = await message.boundingBox();
  expect(messageBox, "the message has no box, so it is not on screen").not.toBeNull();
  expect(
    Math.abs(messageBox.y - fieldBox.y),
    "the refusal is not within sight of the field it is about",
  ).toBeLessThan(200);
  await expect(formulaField(page)).toHaveAttribute("aria-invalid", "true");

  // And the rule was not stored. A rule holding a formula that cannot parse is
  // a highlight that never appears and never says why.
  expect(await cfRules(page)).toEqual([]);

  // An empty formula is refused too, in the same place.
  await formulaField(page).fill("");
  await applyButton(page).click();
  await expect(message).toBeVisible();
  await expect(message).toHaveText(/needs a formula/);
  expect(await cfRules(page)).toEqual([]);

  // And a later success takes the refusal away — in both places. A "does not
  // parse" left standing over a rule that has since applied cleanly is the
  // same failure moved one control along.
  await formulaField(page).fill("=$D2>100");
  await applyButton(page).click();
  await expect(message).toBeHidden();
  await expect(page.locator("#tb-status")).toHaveText("rule applied to B2:E6");
  await expect(formulaField(page)).not.toHaveAttribute("aria-invalid", "true");
});

/// `DV-04` again: a dialog that cannot show what is set can only replace it.
test("reopening the panel shows the rule that is already there", async ({ page }) => {
  await boot(page);
  await seed(page);
  await openCfPanel(page);
  await kindSelect(page).selectOption("formula");
  await formulaField(page).fill("=$D2>100");
  await applyButton(page).click();
  await expect.poll(async () => (await cfRules(page)).length).toBe(1);

  await page.click("#side-panel-close");
  await expect(page.locator("#side-panel")).toBeHidden();
  await openCfPanel(page);

  await expect(kindSelect(page)).toHaveValue("formula");
  await expect(formulaField(page)).toHaveValue("=$D2>100");
  // And it says what Apply will now do. A panel that loads a rule and then
  // silently replaces it on Apply is the mirror of the bug being fixed: the
  // user cannot see which of the two Apply means.
  await expect(panelBody(page).locator(".cf-editing")).toHaveText(
    "Editing the rule already on B2:E6 — Apply rewrites it.",
  );

  // Amending it replaces the rule rather than stacking a second one over it.
  // A stacked rule is evaluated *later*, so the original would keep winning and
  // the edit would appear to do nothing at all.
  await formulaField(page).fill("=$D2>500");
  await applyButton(page).click();
  await expect
    .poll(async () => (await cfRules(page)).map((r) => `${r.range} ${r.desc}`))
    .toEqual(["B2:E6 formula =$D2>500"]);

  const bg = await fills(page);
  expect(bg["1,1"], "D2 is 150, under the new threshold, so row 2 is no longer painted").toBeUndefined();
  expect(bg["3,1"], "D4 is 900, so row 4 still is").toBe("FFD166");
});

// **Out of scope, found here, and worth a row of its own.**
//
// `session_cells` walks the sheet's *stored* cells (`sheet.cells.row_band`), so
// a cell that has never been written is never offered to `effect_for`. A
// whole-row rule therefore paints only the cells of the row that happen to hold
// something: on a table with a blank column, the highlight comes out striped.
// That is a renderer question, not a dialog one, so this file fills every cell
// of its table rather than asserting the striping is correct.
