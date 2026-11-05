// A General number too wide for its column is shortened, not hashed
// (`XL-UX-152`).
//
// Excel: under General, a number that does not fit the column has its decimals
// rounded away or switches to scientific — 3.14159265 in a narrow column shows
// `3.14`, 1234567 shows `1E+06` — and only a number that cannot be shortened at
// all shows `#####`. A fixed format (`0.00`) asked for its digits and keeps
// `#####`. (Microsoft, "Available number formats in Excel": General "rounds
// numbers that have decimals" when the cell is not wide enough, and uses
// scientific notation for large numbers.)
//
// Typing a number widens the column first (`XL-UX-036`), so this only showed
// for values that never pass through entry: import, paste, the API, a formula.
// Those all drew `#####` for a perfectly showable number.
//
// The assertions read what the painter actually drew — `fillText` is recorded
// during one real `draw()` — not a re-derivation of the rule.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.addInitScript(() => {
    const real = CanvasRenderingContext2D.prototype.fillText;
    window.__drawn = null;
    CanvasRenderingContext2D.prototype.fillText = function (text, ...rest) {
      if (window.__drawn) window.__drawn.push(String(text));
      return real.call(this, text, ...rest);
    };
  });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

/// Put `value` in B2 (through the API, as an import or a script would), give
/// column B `width` px and optionally a format, draw once, and return what was
/// drawn in B2's place — the one drawn string that is not a header or label.
async function drawnFor(page, value, width, format) {
  return page.evaluate(([value, width, format]) => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_set_cell(0, 1, 1, value);
    if (format) a.session_set_number_format(0, 1, 1, 1, 1, format);
    a.session_set_col_width(0, 1, width);
    const it = JSON.parse(a.session_cells(0, 1, 1, 1, 1))[0];
    window.__drawn = [];
    ed.draw();
    const drawn = window.__drawn;
    window.__drawn = null;
    return { cell: it.t, drawn };
  }, [value, width, format ?? null]);
}

/// The drawn string for the cell: the full text, a shortened form, or hashes.
const pick = (r, ok) => r.drawn.find((s) => s === r.cell || /^#+$/.test(s) || ok(s));

test("a General decimal too wide for its column is rounded to fit", async ({ page }) => {
  await boot(page);
  const r = await drawnFor(page, "3.14159265", 40);
  const s = pick(r, (s) => /^3(\.\d+)?$/.test(s));
  expect(s, `drawn: ${JSON.stringify(r.drawn.slice(0, 40))}`).toMatch(/^3\.\d+$/);
  expect(s.length).toBeLessThan("3.14159265".length);
});

test("a General integer too wide for its column goes scientific", async ({ page }) => {
  await boot(page);
  const r = await drawnFor(page, "123456789012", 50);
  const s = pick(r, (s) => /E\+11$/.test(s));
  expect(s, `drawn: ${JSON.stringify(r.drawn.slice(0, 40))}`).toMatch(/^1(\.\d+)?E\+11$/);
});

test("a fixed format keeps ##### — it asked for its digits", async ({ page }) => {
  await boot(page);
  const r = await drawnFor(page, "3.14159265", 30, "0.000000");
  const s = pick(r, () => false);
  expect(s, `drawn: ${JSON.stringify(r.drawn.slice(0, 40))}`).toMatch(/^#+$/);
});

test("a General number too wide to shorten at all is still #####", async ({ page }) => {
  await boot(page);
  // 1E+11 needs five characters; a 20px column holds about one.
  const r = await drawnFor(page, "123456789012", 20);
  const s = pick(r, () => false);
  expect(s, `drawn: ${JSON.stringify(r.drawn.slice(0, 40))}`).toMatch(/^#+$/);
});

test("a General number that fits is drawn unchanged", async ({ page }) => {
  await boot(page);
  const r = await drawnFor(page, "3.5", 80);
  expect(r.drawn).toContain("3.5");
});
