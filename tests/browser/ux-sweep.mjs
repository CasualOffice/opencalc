// Measure the editing surface, then write the map from what was measured.
//
// This exists because every prose map in this repository has been caught wrong
// in *both* directions — `docs/47` listed Ctrl+X, Ctrl+Space, F4, Ctrl+D/R and
// Ctrl+; as missing when all of them worked, while `docs/73` recorded five
// keyboard defects of which two had been fixed long before. A map maintained by
// hand drifts, and the drift is invisible: it reads exactly like a map that is
// right.
//
// So nothing here is asserted. Each entry drives the real editor and observes a
// real thing, and the document is generated from the results. A behaviour that
// stops working turns the map red on the next run rather than on the next
// complaint.
//
//   cd tests/browser && node ux-sweep.mjs            # table to stdout
//   cd tests/browser && node ux-sweep.mjs --write    # regenerate the map
//
// It lives here rather than in `tools/` because ESM resolves `@playwright/test`
// from the file's own directory, and this is where Playwright is installed.
//
// It needs the editor served at PORT (default 8123); `tests/browser` starts one.

import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";

const PORT = process.env.PORT || 8123;
// Not `URL`: that shadows the global constructor used just below.
const EDITOR = `http://127.0.0.1:${PORT}/editor.html`;
const MAP = new URL("../../docs/47-UX-AND-FEATURE-MAP.md", import.meta.url).pathname;

/** Each check: drive the editor, then answer one yes/no about what happened. */
const CHECKS = [];
const check = (area, name, run) => CHECKS.push({ area, name, run });

// --- helpers every check can use -------------------------------------------
const seed = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (let r = 0; r < 12; r += 1) {
      for (let c = 0; c < 6; c += 1) {
        a.session_set_cell(0, r, c, `${String.fromCharCode(65 + c)}${r + 1}`);
      }
    }
    window.opencalcEditor.selectForTest(0, 0);
  });

const cell = (page, r, c) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(c) + ed.colWAt(c) / 2, y: ed.rowYAt(r) + ed.rowHAt(r) / 2 };
  }, [r, c]);

// --- the vocabulary ---------------------------------------------------------
// Settled by the first sweep; kept so a regression shows up here rather than in
// somebody's hands.
check("Selection", "drag a column header to reorder", async (page, box, hdr) => {
  const before = await cell(page, 0, 0);
  const a = await centre(page, 0, 0), c = await centre(page, 0, 2);
  await page.mouse.move(box.x + a.x, box.y + hdr.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + c.x, box.y + hdr.h / 2, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 0, 0)) !== before;
});

check("Selection", "drag a row header to reorder", async (page, box, hdr) => {
  const before = await cell(page, 0, 0);
  const a = await centre(page, 0, 0), c = await centre(page, 3, 0);
  await page.mouse.move(box.x + hdr.w / 2, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + hdr.w / 2, box.y + c.y, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 0, 0)) !== before;
});

check("Selection", "drag the selection border to move a range", async (page, box) => {
  const a = await centre(page, 0, 0), t = await centre(page, 5, 3);
  await page.mouse.move(box.x + a.x - 18, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + t.x, box.y + t.y, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 5, 3)) === "A1";
});

check("Selection", "Ctrl+click adds a second range", async (page, box) => {
  const a = await centre(page, 0, 0), d = await centre(page, 4, 4);
  await page.mouse.click(box.x + a.x, box.y + a.y);
  await page.keyboard.down("Control");
  await page.mouse.click(box.x + d.x, box.y + d.y);
  await page.keyboard.up("Control");
  await page.waitForTimeout(120);
  const r = await sel(page);
  return !(r.r0 === 4 && r.c0 === 4 && r.r1 === 4 && r.c1 === 4);
});

check("Selection", "double-click a column border autofits it", async (page, box, hdr) => {
  const w0 = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  const a = await centre(page, 0, 0);
  await page.mouse.dblclick(box.x + a.x + w0 / 2, box.y + hdr.h / 2);
  await page.waitForTimeout(220);
  return (await page.evaluate(() => window.opencalcEditor.colWAt(0))) !== w0;
});

check("Selection", "drag across column headers selects a span", async (page, box, hdr) => {
  const a = await centre(page, 0, 0), c = await centre(page, 0, 3);
  await page.mouse.move(box.x + a.x, box.y + hdr.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + c.x, box.y + hdr.h / 2, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(120);
  return (await sel(page)).c1 >= 3;
});

check("Editing", "drag the fill handle fills", async (page, box) => {
  const a = await centre(page, 0, 0);
  const w = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  const h = await page.evaluate(() => window.opencalcEditor.rowHAt(0));
  await page.mouse.move(box.x + a.x + w / 2 - 2, box.y + a.y + h / 2 - 2);
  await page.mouse.down();
  const t = await centre(page, 4, 0);
  await page.mouse.move(box.x + a.x, box.y + t.y, { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(220);
  return (await cell(page, 4, 0)) !== "";
});

check("Sheets", "double-click a sheet tab renames it", async (page) => {
  await page.locator(".sheet-tab").first().dblclick();
  await page.waitForTimeout(180);
  return page.evaluate(
    () => !!document.querySelector(".sheet-tab input, #sheet-rename, .sheet-tab [contenteditable]"),
  );
});

// --- runner -----------------------------------------------------------------
const browser = await chromium.launch();
const results = [];
for (const c of CHECKS) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const page = await ctx.newPage();
  try {
    await page.goto(EDITOR, { waitUntil: "networkidle" });
    await page.waitForFunction(
      () => document.querySelector("#tb-status")?.textContent?.startsWith("engine v"),
      null,
      { timeout: 30_000 },
    );
    await seed(page);
    await page.waitForTimeout(200);
    const box = await page.locator("#grid").boundingBox();
    const s = await page.evaluate(() => window.opencalcEditor.scrollStateForTest());
    const hdr = { w: s.bodyX0, h: s.bodyY0 };
    results.push({ ...c, verdict: (await c.run(page, box, hdr)) ? "works" : "missing" });
  } catch (why) {
    // A check that cannot run is not a pass. Named so it is fixed, not ignored.
    results.push({ ...c, verdict: "error", why: String(why.message).slice(0, 60) });
  }
  await ctx.close();
}
await browser.close();

const pad = (s, n) => String(s).padEnd(n);
for (const r of results) {
  console.log(`${pad(r.verdict.toUpperCase(), 8)} ${pad(r.area, 11)} ${r.name}${r.why ? "  — " + r.why : ""}`);
}
const missing = results.filter((r) => r.verdict !== "works").length;
console.log(`\n${results.length - missing}/${results.length} present`);

if (process.argv.includes("--write")) {
  const byArea = new Map();
  for (const r of results) byArea.set(r.area, [...(byArea.get(r.area) || []), r]);
  const mark = { works: "✅", missing: "❌", error: "⚠️" };
  let out = `<!-- GENERATED by tests/browser/ux-sweep.mjs — do not edit by hand.

Every prose map in this repository has been caught wrong in both directions.
This one is measured: each row was driven against the real editor and observed.
Regenerate with \`cd tests/browser && node ux-sweep.mjs --write\`, against a
served tree (\`python3 webapp/serve.py 8123\`).
-->

# UX and feature map

${results.length - missing} of ${results.length} measured behaviours present.

`;
  for (const [area, rows] of byArea) {
    out += `## ${area}\n\n| | behaviour |\n|---|---|\n`;
    for (const r of rows) out += `| ${mark[r.verdict]} | ${r.name} |\n`;
    out += "\n";
  }
  writeFileSync(MAP, out);
  console.log(`\nwrote ${MAP}`);
}
process.exit(0);
