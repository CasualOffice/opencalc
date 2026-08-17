// MNT-001: a refused command has to say so.
//
// `webapp/editor.js` reports engine refusals through `statusError(errText(e))`,
// which paints a `.err` span into the status bar. Several user-invoked commands
// wrapped their engine call in a bare `catch {}` instead, so the refusal went
// nowhere: the control ran, nothing changed, and nothing was said. That is the
// same failure the undo path was fixed for — a button that appears to be broken
// rather than a sheet that is protected.
//
// The gate drives the two that are reachable without a mouse on a canvas:
// the Delete key (`clearSelection`) and autofit (`autofitColumnForTest`).
// Both are refused by `guard_protected` on a protected sheet, so protection is
// the lever: it makes the engine throw for a real, product reason rather than
// through a stub.

import { expect, test } from "@playwright/test";

/// Load the editor and expose its module, the way the other editor specs do.
async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, {
    timeout: 30_000,
  });
  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
  });
}

/// Turn on sheet protection, which is what makes the engine refuse.
async function protectSheet(page) {
  await page.evaluate(() => window.__ed.wasmApi().session_set_sheet_protected(0, true));
}

/// The error text currently in the status bar, or "" when the bar holds no
/// error. `.err` is what `statusError` creates and nothing else does.
function statusErr(page) {
  return page.locator("#tb-status .err");
}

test("the Delete key says why a protected sheet refused it", async ({ page }) => {
  await boot(page);
  await protectSheet(page);

  // D2 holds =B2*C2 in the seeded document; if the clear went through, the
  // accessibility mirror would read "empty".
  await page.evaluate(() => window.__ed.selectForTest(1, 3));
  const before = await page.locator("#a11y-1-3").textContent();
  expect(before?.trim(), "the cell starts with a value").not.toBe("");

  await page.locator("#grid").focus();
  await page.keyboard.press("Delete");

  await expect(statusErr(page), "the refusal is on the status bar").toHaveText(
    /protect/i,
  );
  expect(
    (await page.locator("#a11y-1-3").textContent())?.trim(),
    "and the value is still there, which is why silence was wrong",
  ).toBe(before?.trim());
});

// Column width is *not* covered by `guard_protected` (see the report on
// MNT-001), so the lever here is read-only, which `WorkbookSession::edit`
// refuses before it records a step.
test("autofit says why a read-only workbook refused the width", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.__ed.wasmApi().session_set_read_only(true));

  await page.evaluate(() => window.__ed.autofitColumnForTest(0));

  await expect(statusErr(page), "the refusal is on the status bar").toHaveText(
    /reading only/i,
  );
});
