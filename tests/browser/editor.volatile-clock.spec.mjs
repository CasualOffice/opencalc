// `TODAY()` returns today (`CALC-VOL-01`).
//
// The engine reads `Workbook::volatile_now` and never sets it — a calc engine
// that reaches for the wall clock cannot be tested or replayed, and `AGENTS.md`
// puts time in the host. The engine was right about that and has a test for it.
//
// **No host ever supplied the value.** So `TODAY()` returned 0 — rendered as
// 1899-12-30 — and `RAND()` returned one fixed sequence, in every session, in
// every host, for as long as those functions have existed. Any sheet doing date
// arithmetic was wrong, and nothing said so.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.waitForTimeout(300);
}

const evalAt = (page, formulas) => page.evaluate((fs) => {
  const a = window.opencalcEditor.wasmApi();
  a.session_new();
  fs.forEach((f, i) => a.session_set_cell(0, i, 0, f));
  return fs.map((_, i) => JSON.parse(a.session_cells(0, i, 0, i, 0))[0]?.t ?? "");
}, formulas);

test("TODAY is today, and NOW carries the time of day", async ({ page }) => {
  await boot(page);
  const [today, now, year, text] = await evalAt(page, [
    "=TODAY()", "=NOW()", "=YEAR(TODAY())", '=TEXT(TODAY(),"yyyy-mm-dd")',
  ]);

  // The date the *browser* thinks it is, computed the same way a person would
  // read it off a wall — local, not UTC, because a spreadsheet that rolls over
  // at midnight UTC shows yesterday to most of the world for part of every day.
  const expected = await page.evaluate(() => {
    const d = new Date();
    const p = (n) => String(n).padStart(2, "0");
    return { iso: `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`, year: String(d.getFullYear()) };
  });

  expect(text, "TODAY did not return today — the host is not supplying a clock").toBe(expected.iso);
  expect(year).toBe(expected.year);
  expect(Number(today), "TODAY returned the epoch, which is what an unsupplied clock reads").toBeGreaterThan(40000);
  // NOW keeps the fraction TODAY drops; equal values would mean NOW is a date.
  expect(Number(now)).toBeGreaterThan(Number(today));
  expect(Number(now) - Number(today), "NOW is not within the same day as TODAY").toBeLessThan(1);
});

/// **A new session inherits the clock**, or `File ▸ New` returns TODAY to 1899.
///
/// `volatile_now` is `#[serde(skip)]` — correctly, it is environment and not
/// document state — so every fresh session starts at zero. Without the host's
/// last value being remembered, a new workbook shows 1899-12-30 until some
/// unrelated edit happens to refresh it.
test("a workbook created after boot still knows what day it is", async ({ page }) => {
  await boot(page);
  const [first] = await evalAt(page, ['=TEXT(TODAY(),"yyyy")']);
  // A second session, created well after the clock was last supplied.
  const [second] = await evalAt(page, ['=TEXT(TODAY(),"yyyy")']);
  expect(second, "a session created after boot lost the clock").toBe(first);
  expect(Number(second)).toBeGreaterThan(2000);
});

/// **RAND rerolls when the document is recalculated**, which it could not do
/// from a seed nothing ever changes.
///
/// Through `recalculateNow`, which is the route a user takes: it refreshes the
/// clock and the seed first, because an explicit recalculation is exactly when
/// Excel rerolls `RAND`. Creating sessions directly would bypass that and prove
/// nothing about the application — an earlier version of this test did, and
/// reported a failure that was its own doing.
test("RAND rerolls on recalculation", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "=RAND()");
  });
  await page.waitForTimeout(200);

  const draw = () => page.evaluate(() => {
    window.opencalcEditor.recalculateNow();
    return JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, 0, 0, 0, 0))[0]?.t ?? "";
  });

  const seen = new Set();
  for (let i = 0; i < 3; i += 1) {
    seen.add(await draw());
    await page.waitForTimeout(40);
  }
  expect(seen.size, `RAND returned the same value on every recalculation: ${[...seen]}`).toBeGreaterThan(1);
});
