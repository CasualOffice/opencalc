// A history that survives a reload (`HIST-03`), stored compressed (`SAVE-13`).
//
// `SAVE-08` built the store host-agnostic and `HIST-01` reached it from the
// editor, but nothing carried it across a reload — a version lived until the
// tab closed and no longer. **A history you lose by pressing F5 is not a
// history**, and that is the whole of what this checks.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 820 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.waitForTimeout(400);
}

const cell = (page, r, c) => page.evaluate(([row, col]) =>
  JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, row, col, row, col))[0]?.t ?? "",
[r, c]);

test("a saved version is still there after a reload, and still restores", async ({ page }) => {
  await boot(page);
  // A named document, because history is keyed by document — an unnamed
  // workbook must not inherit another unnamed one's versions.
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.wasmApi().session_set_cell(0, 0, 0, "before");
    ed.setDocumentName("figures.xlsx");
  });
  await page.waitForTimeout(300);

  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.fill(".hist-name", "the good one");
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(700);   // the write is async
  await expect(page.locator(".hist-row")).toHaveCount(1);

  // Move the document on, then throw the tab away.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "after"));
  await page.waitForTimeout(200);

  await page.reload();
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.setDocumentName("figures.xlsx");
  });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.opencalcEditor.reloadVersionsForTest());
  await page.waitForTimeout(800);

  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.waitForTimeout(300);
  await expect(page.locator(".hist-row"),
    "the version did not survive the reload — the history is session-only").toHaveCount(1);

  // And it is a real snapshot, not just a row in a list.
  await page.click(".hist-restore");
  await page.waitForTimeout(300);
  const confirm = page.locator(".oc-confirm-actions button", { hasText: "Restore" });
  if (await confirm.count()) await confirm.first().click();
  await page.waitForTimeout(600);
  expect(await cell(page, 0, 0),
    "the restored version's bytes did not come back with it").toBe("before");
});

/// **A different document does not inherit another's history.**
test("versions are kept per document", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.wasmApi().session_set_cell(0, 0, 0, "alpha");
    ed.setDocumentName("one.xlsx");
  });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(700);

  // A second document, same tab.
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.setDocumentName("two.xlsx");
  });
  await page.waitForTimeout(200);
  await page.evaluate(() => window.opencalcEditor.reloadVersionsForTest());
  await page.waitForTimeout(700);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.waitForTimeout(300);
  await expect(page.locator(".hist-row"),
    "a second document was shown the first one's versions").toHaveCount(0);
});

/// **The bytes are compressed, and the engine's accounting is not.**
///
/// `SAVE-13`: 17.82 MiB at 300k cells against 1.61 MiB gzipped. The store keeps
/// counting uncompressed bytes on purpose — a budget measured against a codec
/// would hold more than it could restore if the codec ever changed.
test("stored snapshots are compressed", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    // **Few cells, long repetitive strings.** An earlier version wrote 400
    // cells in a loop and timed out: every `session_set_cell` crosses the wasm
    // boundary and schedules a recalc, so the cost was in the round-trips, not
    // in the bytes. Thirty long repeated strings compress just as convincingly
    // and take a second.
    const long = "Widget Corporation Limited, Northern Division, Quarterly ".repeat(8);
    for (let r = 0; r < 30; r += 1) a.session_set_cell(0, r, 0, long);
    ed.setDocumentName("big.xlsx");
  });
  await page.waitForTimeout(400);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.version-history"));
  await page.click(".hist-actions .btn");
  await page.waitForTimeout(1200);

  const seen = await page.evaluate(async () => {
    const db = await new Promise((res, rej) => {
      const r = indexedDB.open("opencalc-versions", 1);
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    const rows = await new Promise((res, rej) => {
      const r = db.transaction("bytes").objectStore("bytes").getAll();
      r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
    });
    return {
      rows: rows.map((x) => ({ codec: x.codec, n: x.data.byteLength ?? x.data.length })),
      // The engine's own accounting, which stays uncompressed on purpose: a
      // budget measured against a codec would hold more than it could restore
      // if the codec ever changed.
      uncompressed: window.opencalcEditor.wasmApi().session_versions_bytes(),
    };
  });
  expect(seen.rows.length, "nothing reached storage").toBeGreaterThan(0);
  expect(seen.rows[0].codec, "the snapshot was not compressed").toBe("gzip");
  expect(seen.rows[0].n,
    `stored ${seen.rows[0].n} bytes against ${seen.uncompressed} uncompressed — not a saving`)
    .toBeLessThan(seen.uncompressed / 2);
});
