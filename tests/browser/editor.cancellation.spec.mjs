// The only shipped host must be able to stop a long job (`SEC-017`).
//
// `SEC-012` made admission and full recalculation cancellable — the `Cancel`
// trait, `WorkbookSession::open_cancellable`, `recalculate_cancellable` — and
// **no host took the seam**. So the scenario `casual_calc_model::cancel`'s own
// header describes, *a workbook inside every limit and simply enormous holding
// the only thread the browser has until it finishes*, was still exactly what a
// browser user got. A capability can be fully built, fully tested through the
// SDK, and reach nobody.
//
// These assert the seam from the **editor**, because that is where it was
// missing. Each drives a workbook past the engine's cancellation check interval
// (4,096 units — a smaller one finishes before the engine ever asks), runs the
// job under a zero-millisecond budget, and then presses "Keep waiting" to prove
// the limit is an offer rather than a refusal.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// More cells than one cancellation check interval.
///
/// The engine asks whether to stop every 4,096 units of work, deliberately — a
/// question per cell costs a virtual call on a path that is a few instructions.
/// So a sheet smaller than that is not "quick", it is *unstoppable*, and a test
/// built on one would pass whatever this bridge did.
const OVER_ONE_INTERVAL = 6000;

/// **A recalculation past its budget stops, and says the sheet is stale.**
///
/// The failure this replaces is silent: F9 on a large workbook froze the tab,
/// and there was no outcome to report because `session_recalculate` returned
/// nothing at all.
test("F9 past its budget stops, and says the values are out of date", async ({ page }) => {
  await boot(page);

  await page.evaluate(async (rows) => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    // Formula cells, not values: the recalculation budget counts evaluations.
    w.session_paste_tsv(0, 0, 0, Array.from({ length: rows }, (_, i) => `=1+${i}`).join("\n"));
    // Zero, so the deadline has already passed by the first ask. Provoking the
    // real five-second limit would mean a five-second test and a workbook big
    // enough to need one.
    ed.setTimeBudgetsForTest(-1, 0);
  }, OVER_ONE_INTERVAL);

  await page.locator("#grid").focus();
  await page.keyboard.press("F9");

  await expect(
    page.locator("#tb-status .err"),
    "F9 reported a finished calculation on a pass that was stopped part-way",
  ).toHaveText(/calculation stopped/i);

  // The workbook agrees, which is the half that matters: a cancelled pass keeps
  // what it computed, so the sheet is a mixture of fresh and stale values and
  // must still report itself as needing a calculation.
  expect(
    await page.evaluate(async () => {
      const ed = await import(window.__editorModule);
      return ed.wasmApi().session_needs_recalculation();
    }),
    "a stopped recalculation left the workbook claiming it was up to date",
  ).toBe(true);

  // **And the limit is an offer, not a refusal.** A workbook that genuinely
  // takes ten seconds is still the user's workbook.
  await expect(page.locator("#keep-waiting")).toBeVisible();
  await page.locator("#keep-waiting").click();
  await expect(page.locator("#tb-status")).toHaveText(/recalculated/);
  expect(
    await page.evaluate(async () => {
      const ed = await import(window.__editorModule);
      return ed.wasmApi().session_needs_recalculation();
    }),
    "the unlimited retry still did not finish the calculation",
  ).toBe(false);
});

/// **The stateless helpers are stoppable too.**
///
/// `describe_xlsx` and `render_xlsx` are what the landing page hands a
/// visitor's own file to. They admit a package on the same one thread, so
/// leaving them out would have left "the landing page can be frozen by a large
/// file and the editor cannot" — which is the shape this row came in as, one
/// host short.
test("the landing page's own importer can be stopped as well", async ({ page }) => {
  await boot(page);

  const at = await page.evaluate(async (rows) => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    w.session_paste_tsv(0, 0, 0, Array.from({ length: rows }, (_, i) => `big-${i}`).join("\n"));
    const bytes = w.session_save();

    // Straight at the bridge: these helpers have no session and no editor
    // around them, which is the whole point of them.
    w.session_set_time_budget_ms(0);
    let stopped = null;
    try {
      w.describe_xlsx(bytes);
    } catch (err) {
      stopped = String(err && err.message ? err.message : err);
    }
    w.session_clear_time_budget();
    // The same bytes with no limit, so the failure above is the limit and not
    // the file.
    const described = w.describe_xlsx(bytes);
    return { stopped, described };
  }, OVER_ONE_INTERVAL);

  expect(at.stopped, "describe_xlsx ran to completion under a zero budget").not.toBeNull();
  // By code, not by prose: `OC-IMP-0007` is the stable name for "the caller
  // asked for this import to stop".
  expect(at.stopped).toContain("OC-IMP-0007");
  expect(at.described, "the same bytes did not import without a limit").toMatch(/populated cell/);
});

/// **An open past its budget loads nothing, and leaves what was on screen.**
///
/// Fail-closed on purpose: a half-built workbook under the old one's name is
/// worse than no workbook, so the previous session has to survive intact.
test("an open past its budget loads nothing and leaves the workbook alone", async ({ page }) => {
  await boot(page);

  const at = await page.evaluate(async (rows) => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    // A real package, built here rather than committed: admission charges one
    // unit per non-blank cell, so the file has to be genuinely large to be
    // stoppable at all.
    w.session_paste_tsv(0, 0, 0, Array.from({ length: rows }, (_, i) => `big-${i}`).join("\n"));
    const bytes = w.session_save();

    // A landmark that must still be there afterwards.
    w.session_new();
    w.session_set_cell(0, 0, 0, "untouched");

    ed.setTimeBudgetsForTest(0, -1);
    const ok = ed.openBytes(bytes, "enormous.xlsx");
    return { ok, a1: w.session_cell_input(0, 0, 0) };
  }, OVER_ONE_INTERVAL);

  expect(at.ok, "a stopped open reported success").toBe(false);
  expect(at.a1, "a stopped open replaced the workbook that was already loaded").toBe("untouched");
  await expect(
    page.locator("#tb-status"),
    "the file was blamed for being unreadable when it was merely large",
  ).toHaveText(/taking too long/i);

  // Keep waiting: the same bytes, no limit, and now it loads.
  await expect(page.locator("#keep-waiting")).toBeVisible();
  await page.locator("#keep-waiting").click();
  await expect(page.locator("#tb-status")).toHaveText(/opened enormous\.xlsx/);
  expect(
    await page.evaluate(async () => {
      const ed = await import(window.__editorModule);
      return ed.wasmApi().session_cell_input(0, 0, 0);
    }),
    "the unlimited retry did not load the workbook either",
  ).toBe("big-0");
});
