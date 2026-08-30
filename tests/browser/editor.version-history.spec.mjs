// A document with a past a user can reach (`HIST-01`).
//
// The engine and SDK half was built by `SAVE-08` and nothing could reach it:
// undo was the only route backwards, and it dies with the tab. This is the
// largest single feature gap `docs/12` names against Sheets, Excel and
// OnlyOffice, all three of which keep versions.
//
// **A version here is a snapshot, not a replayed log** — `docs/83`'s main
// negative result. The collaboration op log looks like a history and is not
// one: no timestamps, no per-revision author, a few hundred ops retained, the
// session evicted thirty seconds after the last participant leaves (`SAVE-09`).

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 860 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(300);
}

const cell = (page, r, c) => page.evaluate(([row, col]) =>
  JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, row, col, row, col))[0]?.t ?? "",
[r, c]);

test("a version can be saved and the document brought back to it", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "first"));
  await page.waitForTimeout(200);

  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await expect(page.locator("#side-panel")).toBeVisible();
  await expect(page.locator("#side-panel-title")).toHaveText("Version history");

  await page.fill(".hist-name", "before the change");
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(300);
  await expect(page.locator(".hist-row"), "saving a version did not add one to the list").toHaveCount(1);

  // Move the document on.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "second"));
  await page.waitForTimeout(200);
  expect(await cell(page, 0, 0)).toBe("second");

  // And come back. The confirmation is part of the feature, not an obstacle to
  // route around: a restore that did not say what it would change would be the
  // defect, so the test presses the same button a user does.
  page.once("dialog", (d) => d.accept());
  await page.click(".hist-restore");
  await page.waitForTimeout(300);
  const confirm = page.locator(".oc-confirm-actions button", { hasText: "Restore" });
  if (await confirm.count()) await confirm.first().click();
  await page.waitForTimeout(500);

  expect(await cell(page, 0, 0), "the document was not brought back to the saved version").toBe("first");
});

/// **A restore costs exactly one undo step.**
///
/// It arrives as one `Operation::Batch` of ordinary edits, and a batch has one
/// combined inverse — which is what lets it travel to collaborators as edits
/// rather than as a special message, and what stops a restore of a large sheet
/// burying the undo stack.
test("a restore is one undo step, and undo puts the document back", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "alpha");
    a.session_set_cell(0, 1, 0, "beta");
  });
  await page.waitForTimeout(200);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(300);

  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "CHANGED");
    a.session_set_cell(0, 1, 0, "ALSO CHANGED");
  });
  await page.waitForTimeout(200);

  await page.click(".hist-restore");
  await page.waitForTimeout(300);
  const confirm = page.locator(".oc-confirm-actions button", { hasText: "Restore" });
  if (await confirm.count()) await confirm.first().click();
  await page.waitForTimeout(500);
  expect(await cell(page, 0, 0)).toBe("alpha");
  expect(await cell(page, 1, 0)).toBe("beta");

  // One step, not two — both cells came back together.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_undo());
  await page.waitForTimeout(300);
  expect(await cell(page, 0, 0), "undoing the restore left half of it applied").toBe("CHANGED");
  expect(await cell(page, 1, 0)).toBe("ALSO CHANGED");
});

/// **Capturing an unchanged document writes nothing, and says so.**
test("saving a version twice with no edit between does not store two", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "x"));
  await page.waitForTimeout(200);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(250);
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(250);
  await expect(page.locator(".hist-row"),
    "an unchanged document was captured twice, so the list grows while nothing changes").toHaveCount(1);
  await expect(page.locator("#tb-status")).toHaveText(/no changes since the last version/);
});
