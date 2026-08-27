// A dropdown whose options come from a range — which is most of them.
//
// Excel's lists are usually a range kept out of the way and maintained on its
// own, not an inline CSV. The importer preserved the reference and said so:
// "the rule survives even though the editor cannot offer the dropdown yet."
// Both the chevron and the enforcement gated on the literal values being
// non-empty, so the user opened their workbook, the dropdowns were gone, and
// nothing said why.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

test("a range-backed dropdown draws its chevron and opens onto the range", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    ["North", "South", "East"].forEach((v, i) => a.session_set_cell(0, i, 5, v));
    a.session_set_list_validation_range(0, 10, 0, 14, 0, "$F$1:$F$3");
  });

  // The chevron is drawn from this, so a null here is a cell with no dropdown.
  const offered = await page.evaluate(() =>
    JSON.parse(window.opencalcEditor.wasmApi().session_validation_at(0, 10, 0) || "null"),
  );
  expect(offered).toEqual(["North", "South", "East"]);
});

test("the rule is enforced, not merely offered", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    ["North", "South"].forEach((v, i) => a.session_set_cell(0, i, 5, v));
    a.session_set_list_validation_range(0, 10, 0, 10, 0, "$F$1:$F$2");
    window.opencalcEditor.selectForTest(10, 0);
  });

  // A dropdown that accepts anything typed over it is a suggestion, not a
  // validation — the phrase the enforcement path already uses about itself.
  const refused = await page.evaluate(() => window.opencalcEditor.commit("Westeros", false));
  expect(refused, "an off-list value is refused").toBe(false);
  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 10, 0)),
  ).toBe("");

  const accepted = await page.evaluate(() => window.opencalcEditor.commit("South", false));
  expect(accepted, "a value from the range is accepted").toBe(true);
});

test("the list is live: editing the source changes what is allowed", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 5, "Draft");
    a.session_set_list_validation_range(0, 10, 0, 10, 0, "$F$1:$F$2");
    window.opencalcEditor.selectForTest(10, 0);
  });
  expect(await page.evaluate(() => window.opencalcEditor.commit("Final", false))).toBe(false);

  // Adding a row to the source adds an option. That liveness is the whole
  // reason to back a list with a range rather than copy the values into it.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 1, 5, "Final"));
  expect(await page.evaluate(() => window.opencalcEditor.commit("Final", false))).toBe(true);
});

test("the validation dialog accepts a range in the same field as a list", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    ["Alpha", "Beta"].forEach((v, i) => a.session_set_cell(0, i, 5, v));
    window.opencalcEditor.selectForTest(12, 0);
    window.opencalcEditor.runCommand("data.data-validation");
  });

  const field = page.locator("#side-panel-body input").first();
  await expect(field).toBeVisible();
  // Excel spells a range source with a leading `=`, in the same box that takes
  // a comma list. Splitting on commas turned "=$F$1:$F$2" into one literal
  // option whose text was the formula.
  await field.fill("=$F$1:$F$2");
  await page.locator("#side-panel-body button", { hasText: /^Apply$/ }).click();

  await expect
    .poll(() =>
      page.evaluate(() =>
        JSON.parse(window.opencalcEditor.wasmApi().session_validation_at(0, 12, 0) || "null"),
      ),
    )
    .toEqual(["Alpha", "Beta"]);
});
