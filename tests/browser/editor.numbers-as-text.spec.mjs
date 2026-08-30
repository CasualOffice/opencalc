// A number stored as text, said out loud (`DATA-NT-01`).
//
// Reported from the desktop app: "no formulas are working, I tried, SUM
// resulted zero." The engine was right and the report was fair. `SUM` over a
// column of text returns 0 — which is exactly what Excel returns — and nothing
// anywhere said the cells were text. A person reading that zero cannot tell a
// correct empty sum from a column an importer turned into strings.
//
// The engine keeps `"10"` as text on purpose: coercing on the way in would
// silently change somebody's data, and `="10"<"9"` is `TRUE` for a string. So
// the fix is not to change the arithmetic. It is to say so, and to offer the
// conversion Excel offers.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

/** Write text that looks like a number, by the two routes that really produce it.
 *
 * `session_set_cell` re-parses, so a bare `10` becomes a number — which is why
 * an earlier version of this file seeded `="10"` and proved nothing: that is a
 * *formula*, and formulas are deliberately not flagged.
 *
 * The two routes a person actually meets are a column formatted as Text before
 * anything is typed into it, and a leading apostrophe. Both are checked, so a
 * change that fixed one and missed the other could not pass. */
const seedTextNumbers = (page) => page.evaluate(() => {
  const a = window.opencalcEditor.wasmApi();
  a.session_set_number_format(0, 0, 0, 0, 0, "@");   // Text format, then a number
  a.session_set_cell(0, 0, 0, "10");
  a.session_set_cell(0, 1, 0, "'20");                // leading apostrophe
  a.session_set_number_format(0, 2, 0, 2, 0, "@");
  a.session_set_cell(0, 2, 0, "30");
});

const cells = (page, r0, c0, r1, c1) => page.evaluate(([a, b, c, d]) =>
  JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, a, b, c, d)),
[r0, c0, r1, c1]);

/// **The arithmetic is not changed, and this test says so first.**
///
/// If a later change made `SUM` coerce text, that would be a bigger bug than
/// the one being fixed here — silently different answers from Excel — so the
/// behaviour is pinned before the cue is tested.
test("SUM still skips text, as Excel does", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "10");
    a.session_set_cell(0, 1, 0, "20");
    a.session_set_cell(0, 2, 0, "'30");   // text, by apostrophe
    a.session_set_cell(0, 4, 0, "=SUM(A1:A3)");
  });
  await page.waitForTimeout(300);
  const [sum] = await cells(page, 4, 0, 4, 0);
  expect(sum.t, "SUM changed its answer — text must stay out of the arithmetic").toBe("30");
});

/// **A cell that is a number stored as text says so.**
test("a number stored as text is flagged, and a real number is not", async ({ page }) => {
  await boot(page);
  await seedTextNumbers(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 3, 0, "40"));
  await page.waitForTimeout(300);

  const got = await cells(page, 0, 0, 3, 0);
  expect(got.filter((c) => c.nt).length, "the three text numbers are not flagged").toBe(3);
  expect(got[3].nt, "a real number was flagged as text").toBeFalsy();
});

/// **And a label is not.** The flag has to be narrow or it is noise: a column of
/// names would sprout a marker on every cell.
test("ordinary text is not flagged", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "Widget");
    a.session_set_cell(0, 1, 0, "'  ");
    a.session_set_cell(0, 2, 0, "'NaN");
    a.session_set_cell(0, 3, 0, "'1,200");  // a thousands separator is not a number here
  });
  await page.waitForTimeout(300);
  const got = await cells(page, 0, 0, 3, 0);
  expect(got.filter((c) => c.nt).length,
    `something that is not a number was flagged: ${JSON.stringify(got.map((c) => [c.t, c.nt]))}`).toBe(0);
});

/// **Converting them makes the sum right, in one undo step.**
test("Convert text to numbers fixes the sum, and undo puts it back", async ({ page }) => {
  await boot(page);
  await seedTextNumbers(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 4, 0, "=SUM(A1:A3)"));
  await page.waitForTimeout(300);
  expect((await cells(page, 4, 0, 4, 0))[0].t, "the sum should start at 0 — that is the reported symptom").toBe("0");

  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    return window.opencalcEditor.wasmApi().session_convert_text_to_numbers(0, 0, 0, 2, 0);
  });
  await page.waitForTimeout(400);
  expect((await cells(page, 4, 0, 4, 0))[0].t, "converting did not make the sum add up").toBe("60");
  expect((await cells(page, 0, 0, 2, 0)).filter((c) => c.nt).length, "the markers survived the conversion").toBe(0);

  // One batch: converting four hundred cells and then pressing Ctrl+Z four
  // hundred times is not an undo.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_undo());
  await page.waitForTimeout(400);
  expect((await cells(page, 4, 0, 4, 0))[0].t, "one undo did not put all three cells back").toBe("0");
});

/// **A formula is left alone.** Its result is the author's choice, and Excel
/// flags constants rather than results.
test("a formula returning text is neither flagged nor converted", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, '=TEXT(10,"0")');
  });
  await page.waitForTimeout(300);
  expect((await cells(page, 0, 0, 0, 0))[0].nt, "a formula's result was flagged").toBeFalsy();
  const n = await page.evaluate(() =>
    window.opencalcEditor.wasmApi().session_convert_text_to_numbers(0, 0, 0, 0, 0));
  expect(n, "a formula was rewritten as a literal").toBe(0);
});
