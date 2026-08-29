// What a table looks like: its outline, and its header labels.
//
// Two defects the user reported by hand, both invisible to every test we had,
// because both are about pixels rather than about whether a command ran:
//
//  1. `session_table_at` resolves a border colour for the table's style and
//     nothing drew it. Header fill, band fill and body fill were all painted;
//     the boundary was not, so a table read as a few shaded rows rather than as
//     an object. The rule under the header row *was* drawn, which is most of
//     why this looked finished.
//  2. The header label was laid out against the whole cell while the filter
//     arrow was drawn in its right-hand corner, so on a 64px column `Revenue`
//     was drawn underneath its own control and read as `Revenu` with the arrow
//     on the last letter.
//
// Both are asserted here as ink on the canvas, in the same terms the visual
// audit measures them (`tests/browser/ux-visual-audit.mjs`): the audit is a
// report, and a report nobody runs is not a gate.

import { expect, test } from "@playwright/test";

/// A sheet with a table on it, made through the editor's own dialog.
async function bootTable(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    // `Revenue` is the label that did not fit; the rest give the table a shape.
    ["Region", "Rep", "Units", "Revenue"].forEach((h, c) => a.session_set_cell(0, 0, c, h));
    for (let r = 1; r < 9; r += 1) {
      a.session_set_cell(0, r, 0, ["North", "South", "East", "West", "Mid", "Far", "Near", "Top"][r - 1]);
      a.session_set_cell(0, r, 1, `Rep ${r}`);
      a.session_set_cell(0, r, 2, String(r * 7));
      a.session_set_cell(0, r, 3, String(r * 133));
    }
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(300);
  await page.evaluate(() => window.opencalcEditor.tableDialog());
  await page.locator("#oc-modal .oc-btn.primary").click();
  await page.waitForTimeout(500);
  // Off the table, so the selection outline is nowhere near the edges measured.
  await page.evaluate(() => window.opencalcEditor.selectForTest(20, 8));
  await page.waitForTimeout(300);
}

const tableAt = (page) =>
  page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_table_at(0, 0, 0)));

test("a table is drawn with its own outline", async ({ page }) => {
  await bootTable(page);
  const t = await tableAt(page);
  expect(t.border, "the engine resolves a border colour for the style").toMatch(/^[0-9A-F]{6}$/i);

  const left = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const cv = document.getElementById("grid");
    const g = cv.getContext("2d", { willReadFrequently: true });
    const dpr = cv.width / parseFloat(getComputedStyle(cv).width);
    const t = JSON.parse(ed.wasmApi().session_table_at(0, 0, 0));
    const [br, bg, bb] = [0, 2, 4].map((i) => parseInt(t.border.slice(i, i + 2), 16));
    const x = Math.round(ed.colXAt(t.c0) * dpr);
    let hits = 0, rows = 0;
    for (let r = t.r0; r <= t.r1; r += 1) {
      const ry = ed.rowYAt(r);
      if (ry === undefined) continue;
      rows += 1;
      const d = g.getImageData(x, Math.round((ry + ed.rowHAt(r) / 2) * dpr), 1, 1).data;
      if (Math.abs(d[0] - br) + Math.abs(d[1] - bg) + Math.abs(d[2] - bb) < 60) hits += 1;
    }
    return { hits, rows };
  });
  expect(left.rows, "the table is on screen at all").toBeGreaterThan(4);
  expect(left.hits, `${left.hits} of ${left.rows} rows carry the border colour on the table's left edge`)
    .toBe(left.rows);
});

test("a header label is not drawn underneath its own filter arrow", async ({ page }) => {
  await bootTable(page);

  const ink = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const cv = document.getElementById("grid");
    const g = cv.getContext("2d", { willReadFrequently: true });
    const dpr = cv.width / parseFloat(getComputedStyle(cv).width);
    const t = JSON.parse(ed.wasmApi().session_table_at(0, 0, 0));
    const [fr, fg, fb] = [0, 2, 4].map((i) => parseInt(t.headerFill.slice(i, i + 2), 16));
    const y = Math.round((ed.rowYAt(t.r0) + ed.rowHAt(t.r0) / 2) * dpr);
    const out = [];
    for (let c = t.c0; c <= t.c1; c += 1) {
      const x0 = ed.colXAt(c), w = ed.colWAt(c);
      if (x0 === undefined) continue;
      // The strip between the label's room and the arrow's own body: glyph ink
      // here means the two are drawn on top of each other.
      let inArrow = 0;
      for (let px = Math.round((x0 + w - 9) * dpr); px < Math.round((x0 + w - 2) * dpr); px += 1) {
        const d = g.getImageData(px, y, 1, 1).data;
        if (Math.abs(d[0] - fr) + Math.abs(d[1] - fg) + Math.abs(d[2] - fb) > 90) inArrow += 1;
      }
      out.push({ col: c, label: t.cols[c - t.c0], inkInArrowZone: inArrow });
    }
    return out;
  });

  expect(ink.length, "the header row is on screen").toBe(4);
  for (const cell of ink) {
    // The arrow itself is drawn from `x + w - 16`, so its own ink is left of
    // this strip: what is measured here is the label reaching into it.
    expect(cell.inkInArrowZone, `${cell.label} draws ${cell.inkInArrowZone}px into its arrow`)
      .toBeLessThanOrEqual(2);
  }
});

/// **The arrow is not dropped to make room.**
///
/// The guard this replaced skipped the arrow on any column under 22px, under a
/// comment claiming it was protecting the label. Excel and Sheets shorten the
/// label and keep the control, because a header you cannot filter is worse than
/// one you cannot read in full.
test("a narrow header column keeps its filter arrow", async ({ page }) => {
  await bootTable(page);
  await page.evaluate(() => {
    // 20px: under the 22px the old guard silently dropped the arrow at, and
    // over the 18px a 12px glyph with its margin actually needs.
    window.opencalcEditor.wasmApi().session_set_col_width_range(0, 0, 3, 20);
    window.opencalcEditor.draw();
  });
  await page.waitForTimeout(300);
  const buttons = await page.evaluate(() => window.opencalcEditor.filterButtons.length);
  expect(buttons, "every header column still offers its filter").toBe(4);
});
