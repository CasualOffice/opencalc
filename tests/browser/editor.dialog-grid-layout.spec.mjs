// Dialog forms keep both of their columns, and a swatch shows its style.
//
// `.oc-table-form` computed `450px 0px` — the control column collapsed to
// nothing — because `err.hidden` is `display: none` and a `display: none`
// child occupies no grid cell. Every later control shifted one place, the
// full-width checkbox landed in the `auto` label column and swelled it to the
// whole width, and the range input rendered as an 18px pill against the right
// edge with "Style" clipped off it. The form looked broken in its *default*
// state, because the error is hidden until the range is refused.
//
// Test 4 is the general gate rather than a second test of this one dialog: the
// trap is a property of auto-placed grids, not of the create-table form, and
// the next person adding a conditional row to any dialog would meet it again.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    const hdr = ["Region", "Rep", "Units", "Revenue"];
    hdr.forEach((h, c) => a.session_set_cell(0, 0, c, h));
    for (let r = 1; r < 9; r += 1) {
      hdr.forEach((_, c) => a.session_set_cell(0, r, c, c < 2 ? `v${r}${c}` : String(r * 10 + c)));
    }
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(250);
}

const openTable = async (page) => {
  await page.evaluate(() => window.opencalcEditor.tableDialog());
  await page.waitForTimeout(350);
};

test("the create-table form keeps both of its columns", async ({ page }) => {
  await boot(page);
  await openTable(page);
  const cols = await page.evaluate(() =>
    getComputedStyle(document.querySelector(".oc-table-form")).gridTemplateColumns);
  const tracks = cols.split(/\s+/).map(parseFloat);
  expect(tracks.length, `tracks were ${cols}`).toBe(2);
  expect(tracks[1], `control column collapsed: tracks were ${cols}`).toBeGreaterThan(200);
  // And the label column is a label's width, not the whole form.
  expect(tracks[0], `label column swallowed the form: ${cols}`).toBeLessThan(200);
});

test("every control in the create-table form is in the control column", async ({ page }) => {
  await boot(page);
  await openTable(page);
  const m = await page.evaluate(() => {
    const form = document.querySelector(".oc-table-form");
    const fb = form.getBoundingClientRect();
    const at = (sel) => {
      const r = form.querySelector(sel).getBoundingClientRect();
      return { x: Math.round(r.x - fb.x), w: Math.round(r.width) };
    };
    const styleLabel = [...form.children].find((c) => c.textContent === "Style");
    return {
      input: at("#oc-table-range"),
      check: at(".oc-check"),
      styleLabelX: Math.round(styleLabel.getBoundingClientRect().x - fb.x),
      formW: Math.round(fb.width),
    };
  });
  // The input fills the control column rather than being a pill at the edge.
  expect(m.input.w, "the range input is a pill").toBeGreaterThan(m.formW / 2);
  // The checkbox and the input start at the same place: both are controls.
  expect(Math.abs(m.check.x - m.input.x), "the checkbox is not in the control column").toBeLessThan(3);
  // "Style" is a label, so it belongs at the left edge, not off the right one.
  expect(m.styleLabelX, "the Style label is not in the label column").toBe(0);
});

test("a table-style swatch draws its four bands", async ({ page }) => {
  await boot(page);
  await openTable(page);
  const bands = await page.evaluate(() => {
    const sw = document.querySelector(".oc-style-swatch");
    return [...sw.children].map((c) => ({
      h: Math.round(c.getBoundingClientRect().height),
      bg: getComputedStyle(c).backgroundColor,
    }));
  });
  expect(bands.length).toBe(4);
  for (const b of bands) {
    expect(b.h, `band heights ${JSON.stringify(bands)}`).toBeGreaterThan(3);
    // A transparent band shows the panel through it, which is what made every
    // swatch look alike.
    expect(b.bg, `a band is transparent: ${JSON.stringify(bands)}`).not.toBe("rgba(0, 0, 0, 0)");
  }
});

test("no dialog grid has a hidden child in its auto-flow", async ({ page }) => {
  await boot(page);
  const dialogs = [
    "tableDialog", "sortDialog", "documentPropertiesDialog", "customFormatDialog",
    "conditionDialog", "hyperlinkDialog", "textToColumnsDialog", "formatCellsDialog",
    "pasteSpecialDialog", "sizeDialog",
  ];
  let seen = 0;
  const bad = [];
  for (const d of dialogs) {
    await page.evaluate((n) => { try { window.opencalcEditor[n](); } catch { /* not all take no args */ } }, d);
    await page.waitForTimeout(220);
    const r = await page.evaluate(() => {
      const out = { grids: 0, bad: [] };
      for (const el of document.querySelectorAll("#oc-modal *, .side-panel *")) {
        if (getComputedStyle(el).display !== "grid") continue;
        out.grids += 1;
        const dead = [...el.children].filter((c) => getComputedStyle(c).display === "none").length;
        if (dead) {
          out.bad.push(`${el.className || el.tagName}: ${dead} display:none child(ren)`);
        }
      }
      return out;
    });
    seen += r.grids;
    bad.push(...r.bad.map((b) => `${d}: ${b}`));
    await page.keyboard.press("Escape");
    await page.waitForTimeout(120);
  }
  // A sweep that looked at nothing must not read as a pass.
  expect(seen, "no grids were measured, so this proved nothing").toBeGreaterThan(8);
  expect(bad, "a hidden child in an auto-placed grid shifts every later child").toEqual([]);
});
