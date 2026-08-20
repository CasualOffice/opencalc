// Typing a number too large for a double (WASM-02).
//
// `f64::from_str` accepts `1e400` and answers `inf`, so the editor stored
// `Number(inf)`. There is no `.xlsx` or CSV spelling of infinity that reads
// back as a number, so the value left the editor as a number and returned as
// text — a cell that changes kind by being saved.
//
// A browser test because the defect is in the *typing* path. No reader can
// produce this input, so no round-trip fuzzer can reach it — which is why it
// needed a person to notice, and why it needs a test here rather than there.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

async function type(page, ref, text) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}

const input = (page, row, col) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [row, col]);

/// **A value too large for a double is not stored as infinity.**
///
/// It is kept as what was typed, which is what the reader does with anything
/// it will not type as a number — so the person sees their own text back
/// rather than a number they did not write.
test("a number larger than f64 is never stored as infinity", async ({ page }) => {
  await boot(page);
  await type(page, "A1", "1e400");

  const held = await input(page, 0, 0);
  expect(held, "the cell holds an infinity").not.toMatch(/^-?inf/i);
  expect(held, "what was typed was not kept either").toBe("1e400");
});

/// **It does not change kind by being saved.**
///
/// This is the defect stated exactly. The ordinary number beside it is
/// asserted too, so this cannot pass against a build that mangles both.
test("a too-large value survives a save unchanged", async ({ page }) => {
  await boot(page);
  await type(page, "A1", "1e400");
  await type(page, "B1", "42");

  const before = await Promise.all([input(page, 0, 0), input(page, 0, 1)]);

  await page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    const bytes = api.session_save();
    api.session_open(bytes);
  });

  const after = await Promise.all([input(page, 0, 0), input(page, 0, 1)]);
  expect(after[0], "the too-large value changed by being saved").toBe(before[0]);
  expect(after[1], "the ordinary number beside it was disturbed").toBe("42");
});
