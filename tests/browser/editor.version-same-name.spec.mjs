// Two workbooks with the same name must not destroy each other's history
// (fit-gap XL-FEAT-009, P0).
//
// History is keyed by document name, and every save *replaced* that name's
// manifest with the current session's and swept away bytes it did not name.
// Worse: version ids restart at 1 in every session and stored bytes are never
// rewritten, so the second workbook's "version 1" was silently backed by the
// first workbook's snapshot — restoring it brought back the other document.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.waitForTimeout(300);
}

async function asWorkbook(page, text) {
  await page.evaluate((t) => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.wasmApi().session_set_cell(0, 0, 0, t);
    ed.setDocumentName("same.xlsx");
  }, text);
  await page.waitForTimeout(200);
}

async function saveVersion(page, name) {
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.fill(".hist-name", name);
  await page.click(".hist-actions .oc-btn");
  await page.waitForTimeout(800);
}

test("a second workbook with the same name keeps the first one's versions intact", async ({ page }) => {
  await boot(page);
  await asWorkbook(page, "alpha");
  await saveVersion(page, "from alpha");

  await page.reload();
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await asWorkbook(page, "beta");               // a different file, same name
  await saveVersion(page, "from beta");

  await page.reload();
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await asWorkbook(page, "gamma");
  await page.evaluate(() => window.opencalcEditor.reloadVersionsForTest());
  await page.waitForTimeout(800);

  const versions = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_versions()));
  const names = versions.map((v) => v.name).sort();
  expect(names, "a version was lost").toEqual(["from alpha", "from beta"]);

  // Each name restores its own document, not the other's.
  for (const [name, want] of [["from alpha", "alpha"], ["from beta", "beta"]]) {
    const id = versions.find((v) => v.name === name).id;
    const got = await page.evaluate((vid) => {
      const api = window.opencalcEditor.wasmApi();
      api.session_restore_version(vid, Date.now());
      return api.session_cell_input(0, 0, 0);
    }, id);
    expect(got, `"${name}" restored the wrong document`).toBe(want);
  }
});

// The same, without a reload between them: a second same-named workbook opened
// in the same tab restarts the session's version ids, and its "version 1" must
// not be mapped onto the first workbook's stored snapshot.
test("two same-named workbooks in one tab each restore their own version", async ({ page }) => {
  await boot(page);
  await asWorkbook(page, "alpha");
  await saveVersion(page, "from alpha");
  await asWorkbook(page, "beta");
  await saveVersion(page, "from beta");

  await page.reload();
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await asWorkbook(page, "gamma");
  await page.evaluate(() => window.opencalcEditor.reloadVersionsForTest());
  await page.waitForTimeout(800);
  const versions = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_versions()));
  for (const [name, want] of [["from alpha", "alpha"], ["from beta", "beta"]]) {
    const v = versions.find((x) => x.name === name);
    expect(v, `"${name}" was lost`).toBeTruthy();
    const got = await page.evaluate((vid) => {
      const api = window.opencalcEditor.wasmApi();
      api.session_restore_version(vid, Date.now());
      return api.session_cell_input(0, 0, 0);
    }, v.id);
    expect(got, `"${name}" restored the wrong document`).toBe(want);
  }
});
