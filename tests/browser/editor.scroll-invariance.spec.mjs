// What a cell looks like does not depend on where the sheet is scrolled.
//
// A property test for a *class* (`XL-UX-183`). Two members were found on real
// files one at a time, each by the owner watching a recording: row heights that
// re-grew from the cells on screen (`XL-UX-170`), and a merged banner that took
// its fill from a top-left cell looked up among the window's cells, and so
// painted white once that cell scrolled out (`XL-UX-182`). Both have the same
// shape — something decided from *the window's* cells that belongs to the sheet
// or to a range wider than the window — and a test written for each instance
// says nothing about the next one. This one asks the general question.
//
// The method: build one sheet exercising every feature whose drawing could
// reach outside its own cell (spill in both directions, centre-across, merges
// across rows and columns, thick borders, conditional formatting over a range,
// a table, wrapped and rotated text, notes, links, hidden lines, frozen panes),
// then draw it at many scroll offsets and compare, **cell by cell in sheet
// coordinates**, the pixels of every cell that is fully on screen in each. A
// cell seen at two offsets must be the same picture at both. The comparison is
// exact: a scroll moves the content by a whole number of device pixels (at
// 125% the offsets are multiples of 4 CSS px, i.e. 5 device px), and a canvas
// translated by whole pixels rasterises identically — so any difference at all
// is a difference in what was drawn, not antialiasing.
//
// Excel (Microsoft 365): what a cell shows never depends on scrolling.
// Conditional formats evaluate over the rule's whole "Applies to" range
// (https://support.microsoft.com/office/use-conditional-formatting-to-highlight-information-fb1ae1d5-1b2f-4c37-8fb6-ab2a4dc7f9b1),
// merged cells show the upper-left cell's content and format
// (https://support.microsoft.com/office/merge-and-unmerge-cells-5cbd15d5-9375-4540-907f-c673a93fcedf),
// and text overflows into empty neighbours whether or not the cell holding it
// is on screen.
import { expect, test } from "@playwright/test";

/// The tolerance, and why there is one. Whole-pixel translation makes cell
/// content rasterise identically, so this is **not** antialiasing slack. It is
/// for one artefact, measured: Chromium's rasteriser draws a long 1-2px
/// horizontal line (a gridline, a border) with a seam every 320 *screen* pixels
/// where the line is a few levels lighter (211 → 219 on a gridline, up to ~20
/// on a coloured border). The seam is fixed to the screen rather than to the
/// content, so a line pixel inside a cell changes shade as the cell slides
/// across one. Confirmed by tracing every canvas call at the seam: nothing is
/// drawn there. A seam touches one or two pixels of a line; a defect — a
/// missing fill, glyph or border — moves hundreds of channels by over 100.
/// So a cell differs when any channel moves by more than `TOL_MAX`, or more
/// than `TOL_COUNT` channels move by more than `TOL`.
const TOL = 12;
const TOL_MAX = 48;
const TOL_COUNT = 12;

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

/// The fixture. Every feature sits where scrolling will move its owner — the
/// cell that decides how it looks — out of view while part of it stays in.
async function seed(page, { freeze = null } = {}) {
  await page.evaluate((freeze) => import("/editor.core.js").then((c) => {
    const a = c.wasm;
    c.switchSheet(a.session_add_sheet());
    const s = c.state.sheet;
    const set = (r, col, v) => a.session_set_cell(s, r, col, v);
    // Merged banner along row 1 (XL-UX-182's shape), anchored in column A.
    set(0, 0, "MERGED BANNER TITLE");
    a.session_merge_cells(s, 0, 0, 0, 8);
    a.session_set_fill(s, 0, 0, 0, 8, "C0262D", -1, 0);
    // A merge spanning rows: its anchor row scrolls out before the block does.
    set(3, 1, "TALL MERGE");
    a.session_merge_cells(s, 3, 1, 13, 2);
    a.session_set_fill(s, 3, 1, 13, 2, "F4D03F", -1, 0);
    a.session_set_align(s, 3, 1, 3, 1, "center");
    // Text spilling right from a cell that scrolls out on the left.
    set(15, 0, "A long label in A16 that spills right across a run of empty columns");
    // Right-aligned text spilling left, from a cell that is off the right edge.
    set(16, 19, "A right-aligned label in T17 that spills back to the left");
    a.session_set_align(s, 16, 19, 16, 19, "right");
    // Centred text overflowing both ways.
    set(17, 9, "A centred label in J18 overflowing both of its sides");
    a.session_set_align(s, 17, 9, 17, 9, "center");
    // A spill stopped by a number.
    set(19, 0, "A long label in A20 that a number in D20 has to stop");
    set(19, 3, "5");
    // A spill stopped by an empty merged block.
    set(21, 0, "A long label in A22 that an empty merge in D22:E22 has to stop");
    a.session_merge_cells(s, 21, 3, 21, 4);
    // Centre across selection over B24:L24 — wider than one screen of scroll.
    set(23, 1, "Centred across B24:L24");
    a.session_set_align(s, 23, 1, 23, 11, "centerContinuous");
    // Thick and medium borders whose lines straddle the edge they sit on.
    a.session_set_border(s, 25, 0, 27, 12, "all", "thick", "1F4E79");
    a.session_set_border(s, 29, 2, 29, 14, "bottom", "medium", "7030A0");
    a.session_set_border(s, 30, 5, 34, 5, "right", "thick", "00B050");
    a.session_set_border(s, 32, 7, 32, 9, "all", "double", "000000");
    // Conditional formatting whose answer depends on the whole range.
    for (let r = 0; r < 60; r += 1) {
      set(r, 13, String(r));
      set(r, 14, String((r * 7) % 23));
      set(r, 15, String((r * 13) % 31));
      set(r, 16, String(r % 9));
      set(r, 17, String((r * 5) % 17));
    }
    a.session_add_cf(s, 0, 13, 59, 13, "colorscale", 0, 0, "F8696B,FFEB84,63BE7B", "");
    a.session_add_cf(s, 0, 14, 59, 14, "databar", 0, 0, "638EC6", "");
    a.session_add_cf(s, 0, 15, 59, 15, "top", 5, 0, "", "FFC7CE");
    a.session_add_cf(s, 0, 16, 59, 16, "duplicate", 0, 0, "", "C6EFCE");
    a.session_add_cf(s, 0, 17, 59, 17, "above", 0, 0, "", "FFEB9C");
    // A table: header style, banding, outline.
    for (let r = 36; r <= 52; r += 1) {
      for (let col = 19; col <= 22; col += 1) set(r, col, r === 36 ? `Head ${col}` : String(r * col));
    }
    a.session_create_table(s, 36, 19, 52, 22, "ScrollT", true);
    // Wrapped prose, a line break without wrap, rotated text (XL-UX-170's shape).
    for (let r = 40; r < 50; r += 1) {
      set(r, 1, "a sentence long enough to wrap several times in a default column");
      set(r, 3, `${r} Some Street\nSomewhere`);
    }
    a.session_toggle_wrap(s, 40, 1, 49, 1);
    set(55, 4, "Rotated");
    a.session_set_rotation(s, 55, 4, 55, 4, 45);
    // Notes, a link, a hidden column and a hidden row inside the area.
    a.session_set_comment(s, 2, 6, "a note", "t", "");
    a.session_set_comment(s, 30, 11, "another", "t", "");
    a.session_set_hyperlink(s, 8, 10, "https://example.com", "", "", "link");
    a.session_hide_cols(s, 25, 25);
    a.session_hide_rows(s, 58, 58);
    set(57, 24, "beside the hidden column"); set(57, 26, "and after it");
    // Somewhere to scroll to.
    set(150, 60, "far");
    if (freeze) a.session_set_freeze(s, freeze.rows, freeze.cols);
    // The active cell is far away, so no selection chrome lands in the area.
    c.selectForTest?.(140, 55);
    c.state.scrollX = 0; c.state.scrollY = 0;
    c.draw();
  }), freeze);
  await settle(page);
}

/// Wait until the frame stops changing on its own. The theme's colours are
/// read from CSS custom properties that ease in after load, so for the first
/// second or so the gridlines and headers drift by a level or two per frame —
/// a difference this test would otherwise blame on the scroll.
async function settle(page) {
  await expect.poll(() => page.evaluate(() => import("/editor.core.js").then(async (c) => {
    const cv = document.querySelector("#grid");
    const g = cv.getContext("2d");
    c.state.scrollX = 0; c.state.scrollY = 0;
    c.draw();
    const a = g.getImageData(0, 0, cv.width, cv.height).data;
    await new Promise((r) => setTimeout(r, 150));
    c.draw();
    const b = g.getImageData(0, 0, cv.width, cv.height).data;
    for (let k = 0; k < a.length; k += 1) if (a[k] !== b[k]) return false;
    return true;
  })), { timeout: 10_000, message: "the grid never stopped changing at rest" }).toBe(true);
}

/// Draw at one offset and fingerprint every cell that is fully on screen.
const snapshot = (page, sx, sy) => page.evaluate(([sx, sy, TOL, TOL_MAX, TOL_COUNT]) => import("/editor.core.js").then((c) => {
  c.state.scrollX = sx; c.state.scrollY = sy;
  c.draw();
  const cv = document.querySelector("#grid");
  const g = cv.getContext("2d");
  const scale = (window.devicePixelRatio || 1) * c.state.zoom;
  const W = cv.width, H = cv.height;
  const data = g.getImageData(0, 0, W, H).data;
  const F = c.state.freeze;
  const vw = W / scale, vh = H / scale;
  const HW = c.geo.colX[0] ?? 0, HH = c.geo.rowY[0] ?? 0;
  // Each pane's bounds, in grid units. A cell counts only when it is inside its
  // own pane with a 2-unit margin at a freeze line: the divider is drawn *on*
  // that edge and belongs to the frame, not to the cell.
  const pane = (r, col) => ({
    x0: col < F.fc ? HW : F.bodyX0, x1: col < F.fc ? F.bodyX0 : vw,
    y0: r < F.fr ? HH : F.bodyY0, y1: r < F.fr ? F.bodyY0 : vh,
  });
  const cells = {};
  const diffs = [];
  window.__scrollRef ??= new Map();
  for (let i = 0; i < c.geo.colIdx.length; i += 1) {
    const col = c.geo.colIdx[i], x = c.geo.colX[i], w = c.geo.colW[i];
    if (!(w > 0)) continue;
    for (let j = 0; j < c.geo.rowIdx.length; j += 1) {
      const r = c.geo.rowIdx[j], y = c.geo.rowY[j], h = c.geo.rowH[j];
      if (!(h > 0)) continue;
      const p = pane(r, col);
      // The window's own left/top edge needs no margin — the header strip
      // ends exactly there — and is where a border shared with a cell that
      // has just scrolled out shows up, so it must be in.
      // Only where that edge is a whole device pixel, though: at 125% it is
      // not, and the header's antialiased edge bleeds a sliver into the cell.
      const exact = (e) => Math.abs(e * scale - Math.round(e * scale)) < 1e-6;
      const mx0 = p.x0 === HW && exact(HW) ? 0 : 2, my0 = p.y0 === HH && exact(HH) ? 0 : 2;
      if (x < p.x0 + mx0 || x + w > p.x1 - 2 || y < p.y0 + my0 || y + h > p.y1 - 2) continue;
      const px0 = Math.round(x * scale), py0 = Math.round(y * scale);
      const px1 = Math.round((x + w) * scale), py1 = Math.round((y + h) * scale);
      // Compared against the first frame that showed this cell, in the page:
      // a pixel matches when no channel moved by more than `TOL`.
      const key = r + "," + col;
      const pw = px1 - px0, ph = py1 - py0;
      const buf = new Uint8Array(pw * ph * 3);
      for (let yy = 0, o = 0; yy < ph; yy += 1) {
        let k = ((py0 + yy) * W + px0) * 4;
        for (let xx = 0; xx < pw; xx += 1, k += 4) { buf[o++] = data[k]; buf[o++] = data[k + 1]; buf[o++] = data[k + 2]; }
      }
      const ref = window.__scrollRef.get(key);
      if (!ref) { window.__scrollRef.set(key, { pw, ph, buf, at: [sx, sy] }); cells[key] = 0; continue; }
      if (ref.pw !== pw || ref.ph !== ph) { diffs.push(`${key} size ${ref.pw}x${ref.ph} at ${ref.at} vs ${pw}x${ph}`); continue; }
      let bad = 0, worst = 0, where = null;
      for (let o = 0; o < buf.length; o += 1) {
        const dlt = Math.abs(buf[o] - ref.buf[o]);
        if (dlt > TOL) { bad += 1; if (dlt > worst) { worst = dlt; where = Math.floor(o / 3); } }
      }
      if (bad > TOL_COUNT || worst > TOL_MAX) diffs.push(`${key}: ${bad} channel(s) off by up to ${worst} at (${where % pw},${Math.floor(where / pw)}) vs scroll ${ref.at}`);
      cells[key] = 1;
    }
  }
  // Geometry by index: sizes, and each line's position in *sheet* units (its
  // screen position plus the scroll), which must not depend on the scroll.
  const cols = {}, rows = {};
  c.geo.colIdx.forEach((col, i) => {
    cols[col] = { w: c.geo.colW[i], at: col < F.fc ? c.geo.colX[i] : c.geo.colX[i] + sx };
  });
  c.geo.rowIdx.forEach((r, i) => {
    rows[r] = { h: c.geo.rowH[i], at: r < F.fr ? c.geo.rowY[i] : c.geo.rowY[i] + sy };
  });
  return { sx, sy, cells, diffs, cols, rows, firstCol: c.state.firstCol, firstRow: c.state.firstRow };
}), [sx, sy, TOL, TOL_MAX, TOL_COUNT]);

/// Every pair of frames that both show a cell must agree on it.
function compare(frames) {
  const cellDiffs = [];
  const geoDiffs = [];
  const firstCol = new Map(), firstRow = new Map();
  const seen = new Map();
  for (const f of frames) {
    for (const d of f.diffs) {
      const [r, c] = d.split(/[,:\s]/).map(Number);
      cellDiffs.push(`R${r + 1}C${c + 1} at scroll ${f.sx},${f.sy}: ${d}`);
    }
    for (const [k, n] of Object.entries(f.cells)) seen.set(k, (seen.get(k) ?? 0) + n);
    for (const [k, g] of Object.entries(f.cols)) {
      const p = firstCol.get(k);
      if (!p) firstCol.set(k, { ...g, at0: [f.sx, f.sy] });
      else if (p.w !== g.w || p.at !== g.at) geoDiffs.push(`col ${k}: w/x ${p.w}/${p.at} at ${p.at0} vs ${g.w}/${g.at} at ${f.sx},${f.sy}`);
    }
    for (const [k, g] of Object.entries(f.rows)) {
      const p = firstRow.get(k);
      if (!p) firstRow.set(k, { ...g, at0: [f.sx, f.sy] });
      else if (p.h !== g.h || p.at !== g.at) geoDiffs.push(`row ${k}: h/y ${p.h}/${p.at} at ${p.at0} vs ${g.h}/${g.at} at ${f.sx},${f.sy}`);
    }
  }
  // Cells compared at least once against an earlier frame — the measure of
  // how much the sweep actually covered.
  const compared = [...seen.values()].filter((n) => n > 0).length;
  return { cellDiffs, geoDiffs, compared };
}

/// The offsets: sweeps on each axis, sub-cell and multi-cell steps, diagonals.
function offsets(step) {
  // 384 and 600 put a column / row boundary exactly on the window's edge, where
  // a border drawn on that edge is half in the cell that has just scrolled out.
  const xs = [0, 1, 7, 13, 32, 64, 65, 100, 129, 150, 203, 256, 333, 384, 448, 517, 640, 777, 900];
  const ys = [0, 3, 10, 20, 21, 47, 100, 160, 233, 300, 410, 555, 600];
  const q = (v) => Math.round(v / step) * step;
  const out = [];
  for (const x of xs) out.push([q(x), 0]);
  for (const y of ys) out.push([0, q(y)]);
  for (let i = 0; i < Math.min(xs.length, ys.length); i += 1) out.push([q(xs[i]), q(ys[i])]);
  for (const x of xs) out.push([q(x), q(233)]);
  return out;
}

async function sweep(page, step) {
  await page.evaluate(() => { window.__scrollRef = new Map(); });
  const frames = [];
  for (const [x, y] of offsets(step)) frames.push(await snapshot(page, x, y));
  return compare(frames);
}

for (const zoom of [1, 1.25]) {
  for (const freeze of [null, { rows: 2, cols: 1 }]) {
    const name = `zoom ${zoom * 100}%${freeze ? `, frozen ${freeze.rows} rows/${freeze.cols} col` : ""}`;
    test(`every cell looks the same at every scroll offset (${name})`, async ({ page }) => {
      test.setTimeout(120_000);
      await boot(page);
      await seed(page, { freeze });
      if (zoom !== 1) {
        await page.evaluate((z) => import("/editor.core.js").then((c) => { c.setZoomForTest(z); c.draw(); }), zoom);
        await settle(page);
      }
      // At 125% a whole device pixel is 0.8 grid units, so offsets are kept to
      // multiples of 4 — 5 device pixels — and every scroll is a whole-pixel
      // translation. Anything finer would compare antialiasing, not drawing.
      const r = await sweep(page, zoom === 1 ? 1 : 4);
      expect(r.compared, "the sweep compared too few cells to mean anything").toBeGreaterThan(400);
      expect(r.geoDiffs, "a line's size or sheet position moved with the scroll").toEqual([]);
      expect(r.cellDiffs, "a cell's pixels changed with the scroll").toEqual([]);
    });
  }
}
