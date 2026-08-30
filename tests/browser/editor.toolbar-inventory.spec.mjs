// The toolbar is a fixed budget, and the way to keep it is to govern what goes
// in it (`UX-CHR-06`, `docs/88` §3.3–3.4).
//
// The `Number ⌄` / `Data ⌄` / `Tools ⌄` chips are not a web idiom chosen for
// looks. They are `reflowToolbar()`'s overflow behaviour — written for a phone —
// firing on a desktop, because the bar needed more width than a laptop has.
// `UX-CHR-05`'s metrics bought most of that back: measured here, the bar now
// collapses at 1269px rather than 1461px.
//
// **11px of headroom is not a contract, it is a coincidence.** The next control
// anybody adds spends it, and the collapse mechanism absorbs the overspend
// silently — a chip appears, and nothing fails. That is the actual defect: not
// the chips, but that going over budget has no consequence. So the budget is
// asserted here, and `docs/88`'s ranked list stops being needed to remember it.

import { expect, test } from "@playwright/test";

async function boot(page, width) {
  await page.setViewportSize({ width, height: 900 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.waitForTimeout(250);
}

const chips = (page) => page.evaluate(() =>
  [...document.querySelectorAll(".tb-collapsed")]
    .filter((e) => e.getBoundingClientRect().width > 0)
    .map((e) => e.textContent.replace(/\s+/g, " ").trim()));

// 1280 is the width the contract is written at: it is the narrowest mainstream
// laptop viewport, and the one `docs/88` measured a third of the bar as chips at.
for (const width of [1920, 1600, 1440, 1366, 1280]) {
  test(`no toolbar group is collapsed at ${width}px`, async ({ page }) => {
    await boot(page, width);
    expect(await chips(page), `the toolbar overflowed at ${width}px, so a desktop user is being shown the phone's overflow`).toEqual([]);
  });
}

/// **The chrome could not say what format the active cell has.**
///
/// Every competitor's toolbar carries this readout — Excel and OnlyOffice as a
/// combo, LibreOffice as a listbox, Sheets behind `123 ⌄`. Ours had an icon that
/// opened a menu of formats to *set*, and nothing that reported the one in
/// force, so the only way to learn a cell's format was to change it.
test("the toolbar reports the active cell's number format", async ({ page }) => {
  await boot(page, 1440);
  const readout = page.locator("#tb-numfmt-label");
  await expect(readout, "there is no number-format readout in the toolbar").toBeVisible();
  await expect(readout).toHaveText(/General/i);

  // Through the control a user would use, not by writing the model behind it:
  // the readout is refreshed from the paint tail, so a direct wasm write would
  // leave it stale and the test would be asserting against a frame that never
  // happened.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "1234.5"));
  await page.click("#tb-currency");
  await page.waitForTimeout(250);
  await expect(readout, "the readout did not follow the cell's format").toHaveText(/Currency/i);
});

/// **A menu item that routes through a button is a route, not a listing.**
///
/// Data validation, conditional formatting and Note were listed in the menus and
/// reached by `clickEl("#tb-dv")` — they *clicked the toolbar button*. And
/// `clickEl` is `if (n) n.click()`, so with the group gone all three would have
/// become silent no-ops: the menu item still there, still enabled, doing
/// nothing. `docs/88` §3.4 said the three commands "are already in the menus",
/// which was true of the listing and false of the route.
test("the tools commands survive the group leaving the toolbar", async ({ page }) => {
  await boot(page, 1440);
  expect(await page.locator("#tbg-tools").count(), "the tools group is still on the toolbar").toBe(0);

  // `runCommand` refuses an id the rules have hidden, so a `true` here is a
  // reached command and not a listed one. The panel is then checked in the DOM,
  // because a handler that runs and does nothing returns `true` just as happily.
  for (const [id, title] of [
    ["data.data-validation", /validation/i],
    ["format.conditional-formatting", /conditional/i],
    ["insert.note", /note|comment/i],
  ]) {
    expect(await page.evaluate((c) => window.opencalcEditor.runCommand(c), id),
      `${id} was refused`).toBe(true);
    await expect(page.locator("#side-panel"),
      `${id} ran and opened nothing — it was routed through the deleted toolbar button`).toBeVisible();
    await expect(page.locator("#side-panel-title")).toHaveText(title);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(120);
  }
});
