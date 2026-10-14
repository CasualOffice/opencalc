// A number, date or time entered into a default-width column widens the
// column instead of showing ##### (`XL-UX-036`).
//
// Excel: when an entry or a number-format change makes a numeric value too wide
// for a column whose width the user never set, the column grows to fit it. Text
// does not widen anything (it spills), and a column the user sized is never
// touched — there the number shows #####. The widening is part of the edit: a
// single Ctrl+Z takes back both the value and the width.
//
// OpenCalc showed ##### for Ctrl+; in a fresh sheet, which is the first thing a
// new user tries and reads as "the date is broken".

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
  await page.locator("#grid").focus();
}

const colW = (page, c) =>
  page.evaluate((c) => window.opencalcEditor.wasmApi().session_col_width(0, c), c);

/// What the cell shows and whether it fits, by the renderer's own rule: a
/// number whose text is wider than the column less 8px of padding is drawn as
/// "#". Measured with the renderer's font and its width cache, so this is the
/// number the paint loop compares — not an estimate of it.
const shown = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    const it = JSON.parse(a.session_cells(0, r, c, r, c))[0];
    if (!it) return null;
    const tw = ed.textWidth(ed.cellFont(it), String(it.t));
    const w = a.session_col_width(0, c);
    return { t: it.t, n: !!it.n, tw, w, hashes: !!it.n && tw > w - 8 };
  }, [r, c]);

const input = (page, r, c) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);

const select = (page, r, c) =>
  page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c), [r, c]);

test("Ctrl+; widens a default-width column instead of showing #####, and one undo takes both back", async ({ page }) => {
  await boot(page);
  const before = await colW(page, 1);
  await select(page, 1, 1);
  await page.keyboard.press("Control+Semicolon");
  await expect.poll(() => input(page, 1, 1)).toMatch(/^\d{4}-\d{2}-\d{2}$/);

  const s = await shown(page, 1, 1);
  expect(s.n, `the date is a number: ${JSON.stringify(s)}`).toBe(true);
  expect(s.hashes, `B2 renders as ##### — ${JSON.stringify(s)}`).toBe(false);
  expect(await colW(page, 1), "column B grew").toBeGreaterThan(before);

  // One step: the date and the width it brought with it.
  await page.keyboard.press("Control+z");
  await expect.poll(() => input(page, 1, 1), "undo cleared the date").toBe("");
  expect(await colW(page, 1), "the same undo put the width back").toBe(before);
  // And redo brings both back together.
  await page.keyboard.press("Control+y");
  await expect.poll(() => input(page, 1, 1)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(await colW(page, 1)).toBeGreaterThan(before);
});

test("typing a long number widens; a later, longer one widens again", async ({ page }) => {
  await boot(page);
  const before = await colW(page, 2);
  await select(page, 2, 2);
  await page.keyboard.type("$1,234,567.89");
  await page.keyboard.press("Enter");
  await expect.poll(() => input(page, 2, 2)).not.toBe("");
  const first = await shown(page, 2, 2);
  expect(first.hashes, `C3 renders as ##### — ${JSON.stringify(first)}`).toBe(false);
  const w1 = await colW(page, 2);
  expect(w1).toBeGreaterThan(before);

  // The column was widened by entry, not by the user, so it is still Excel's
  // to widen.
  await select(page, 3, 2);
  await page.keyboard.type("$123,451,234,567.89");
  await page.keyboard.press("Enter");
  await expect.poll(() => input(page, 3, 2)).not.toBe("");
  const second = await shown(page, 3, 2);
  expect(second.hashes, `C4 renders as ##### — ${JSON.stringify(second)}`).toBe(false);
  expect(await colW(page, 2)).toBeGreaterThan(w1);
});

// Ctrl+Shift+^ (scientific), Ctrl+Shift+# (date) and Ctrl+Shift+$ (currency)
// each lengthen what a short number shows. `Digit6`/`Digit3`/`Digit4` are the
// keys; the handler reads the shifted character or the digit.
for (const [label, chord, value, want, col] of [
  ["Ctrl+Shift+^", "Control+Shift+Digit6", "12345", /^1\.23E\+04$/, 4],
  ["Ctrl+Shift+#", "Control+Shift+Digit3", "45000", /^\d{1,2}-[A-Z][a-z]{2}-\d{2}$/, 6],
  ["Ctrl+Shift+$", "Control+Shift+Digit4", "12345", /^\$12,345\.00$/, 7],
]) {
  test(`${label} lengthens the display and widens the column, in one undo step`, async ({ page }) => {
    await boot(page);
    await page.evaluate(([c, v]) => window.opencalcEditor.wasmApi().session_set_cell(0, 1, c, v), [col, value]);
    const before = await colW(page, col);
    await select(page, 1, col);
    expect((await shown(page, 1, col)).hashes, "fits before the format").toBe(false);
    await page.keyboard.press(chord);
    await expect.poll(async () => (await shown(page, 1, col)).t).toMatch(want);
    const s = await shown(page, 1, col);
    expect(s.hashes, `renders as ##### — ${JSON.stringify(s)}`).toBe(false);
    expect(await colW(page, col)).toBeGreaterThan(before);
    await page.keyboard.press("Control+z");
    await expect.poll(async () => (await shown(page, 1, col)).t).toBe(value);
    expect(await colW(page, col), "one undo restores format and width").toBe(before);
  });
}

test("a column the user sized is never widened — it shows ##### as Excel does", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_col_width(0, 3, 40));
  const before = await colW(page, 3);
  await select(page, 1, 3);
  await page.keyboard.press("Control+Semicolon");
  await expect.poll(() => input(page, 1, 3)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  expect(await colW(page, 3), "a user-set width was changed").toBe(before);
  expect((await shown(page, 1, 3)).hashes, "the date should not fit in 40px").toBe(true);
});

test("text never widens a column — it spills", async ({ page }) => {
  await boot(page);
  const before = await colW(page, 5);
  await select(page, 1, 5);
  await page.keyboard.type("a long run of words that is wider than the column");
  await page.keyboard.press("Enter");
  await expect.poll(() => input(page, 1, 5)).not.toBe("");
  expect(await colW(page, 5)).toBe(before);
});
