// A read-only editor stays read-only when it opens a file.
//
// Read-only lived on the engine session, and opening a file (or starting a new
// workbook) installs a new session, which came up editable: a viewer or a
// read-only host that loaded a document with openBytes accepted edits. It is
// the host's setting now, inherited by every session.

import { expect, test } from "@playwright/test";

test("read-only survives opening a file", async ({ page }) => {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  const r = await page.evaluate(async () => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    const bytes = api.session_save();
    ed.setReadOnly(true);
    await ed.openBytes(bytes, "again.xlsx");
    let edit;
    try { api.session_set_cell(0, 0, 0, "edited-after-open"); edit = "accepted"; } catch { edit = "refused"; }
    return { edit, readOnly: api.session_read_only() };
  });
  expect(r.readOnly, "the opened workbook is not read-only").toBe(true);
  expect(r.edit, "an edit to the opened workbook was accepted").toBe("refused");
});
