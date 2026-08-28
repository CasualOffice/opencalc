// Format as Table must ask before it acts.
//
// Reported: "format as table is also not similar to google or so — I want it
// comparable." It was not a dialog at all. It detected a block, immediately
// called session_create_table(..., "", true) — no style, headers assumed — and
// opened a panel. The user got a table they never got to shape: no confirmation
// of the range, no say in whether row 1 was headers, no style.
//
// Excel's Ctrl+T asks all three first. Google's equivalent additionally applies
// a colour scheme immediately, which is why the default here is a real style
// rather than none.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    // From an empty sheet: the editor seeds a demo workbook, and block
    // detection quite correctly found *that* block instead of the fixture's —
    // which is the dialog working and the test assuming.
    a.session_new();
    const rows = [["Region", "Rep", "Units"], ["North", "Ada", "7"], ["South", "Grace", "3"]];
    rows.forEach((row, r) => row.forEach((v, c) => a.session_set_cell(0, r, c, v)));
    window.opencalcEditor.selectForTest(0, 0);
  });
}

const tableAt = (page, r, c) =>
  page.evaluate(
    ([r, c]) => {
      try {
        return JSON.parse(window.opencalcEditor.wasmApi().session_table_at(0, r, c) || "null");
      } catch {
        return null;
      }
    },
    [r, c],
  );

const modal = (page) => page.locator(".oc-modal:not([hidden])");

test("Ctrl+T asks, showing the detected range", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+t");

  await expect(modal(page)).toBeVisible();
  // The range it detected, editable — "where is the data for your table?".
  await expect(page.locator(".oc-modal:not([hidden]) input").first()).toHaveValue(/^A1:C3$/i);
  // And nothing has been created yet.
  expect(await tableAt(page, 0, 0), "asking is not doing").toBeNull();
});

test("cancelling creates nothing", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+t");
  await expect(modal(page)).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(modal(page)).toHaveCount(0);
  // The failure mode of a dialog bolted onto something that used to act
  // immediately: it asks, and creates the table anyway.
  expect(await tableAt(page, 0, 0), "Escape must leave the sheet alone").toBeNull();
});

test("confirming creates the table over the range shown", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+t");
  await expect(modal(page)).toBeVisible();
  await page.locator(".oc-modal:not([hidden]) button", { hasText: /^Create$/ }).click();

  await expect.poll(() => tableAt(page, 0, 0)).not.toBeNull();
  const t = await tableAt(page, 0, 0);
  expect(t.headers ?? t.has_headers ?? true, "headers were ticked").toBeTruthy();
});

test("unticking headers creates a table without them", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+t");
  await expect(modal(page)).toBeVisible();

  await page.locator(".oc-modal:not([hidden]) input[type=checkbox]").first().uncheck();
  await page.locator(".oc-modal:not([hidden]) button", { hasText: /^Create$/ }).click();

  await expect.poll(() => tableAt(page, 0, 0)).not.toBeNull();
  const t = await tableAt(page, 0, 0);
  expect(t.headers ?? t.has_headers, "the checkbox must reach the engine").toBeFalsy();
});

test("an unparseable range is refused visibly and creates nothing", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+t");
  await expect(modal(page)).toBeVisible();

  await page.locator(".oc-modal:not([hidden]) input").first().fill("not a range");
  await page.locator(".oc-modal:not([hidden]) button", { hasText: /^Create$/ }).click();

  // Refused visibly, not ignored silently — and the dialog stays open so the
  // typo can be corrected rather than retyped from the menu.
  await expect(modal(page)).toBeVisible();
  expect(await tableAt(page, 0, 0)).toBeNull();
});
