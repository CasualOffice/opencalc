// Frame timing for the grid, measured while it is actually scrolling.
//
// Not a gate — `editor.frame-budget.spec.mjs` holds the structural assertions,
// because a wall-clock assertion on a shared machine is a flaky test that ends
// up deleted. This is the harness for answering "is it faster", and it exists
// in the tree because the first two attempts at that question both measured
// the wrong thing:
//
//   - The original `PERF-D-01` probe awaited a frame per wheel event while
//     `scheduleDraw` coalesces onto its own rAF, so it inflated the baseline.
//   - This one, first time round, awaited the timing promise *before* starting
//     the scroll — so it measured 1.5s of idle frames and reported a flat
//     16.7ms both before and after the fix. It only discriminates because the
//     scroll now runs during the measurement.
//
// Run with the editor served: `python3 webapp/serve.py 8123`. `OPENCALC_SMOKE_PORT`
// points it at a different one — a worktree serving its own `webapp/` cannot use
// 8123 if the main checkout is already there, and a before/after comparison has
// to run both halves against the same server.
import { chromium } from "@playwright/test";
const PORT = Number(process.env.OPENCALC_SMOKE_PORT ?? 8123);
const b = await chromium.launch();
const page = await b.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto(`http://127.0.0.1:${PORT}/editor.html`);
await page.waitForFunction(() => /^engine v/.test(document.querySelector("#tb-status")?.textContent || ""), null, { timeout: 30000 });
await page.evaluate(() => {
  const a = window.opencalcEditor.wasmApi();
  for (let r = 0; r < 600; r++) for (let c = 0; c < 40; c++) a.session_set_cell(0, r, c, `r${r}c${c}`);
});
for (const w of [102, 30]) {
  await page.evaluate((w) => window.opencalcEditor.wasmApi().session_set_col_width_range(0, 0, 39, w), w);
  await page.waitForTimeout(400);
  const box = await page.locator("#grid").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  // Time real animation frames while scrolling.
  const scrolling = (async () => { for (let i = 0; i < 200; i++) await page.mouse.wheel(0, 30); })();
  const ms = await page.evaluate(async () => {
    const t = [];
    let last = performance.now();
    const stop = performance.now() + 1500;
    return await new Promise((res) => {
      const tick = () => {
        const now = performance.now();
        t.push(now - last); last = now;
        if (now < stop) requestAnimationFrame(tick); else res(t);
      };
      requestAnimationFrame(tick);
    });
  });
  await scrolling;
  const w2 = await page.evaluate(() => window.opencalcEditor.frameWindowForTest());
  const sorted = ms.slice(5).sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))];
  // The median alone cannot see work that happens a few times a second: the
  // grid is vsync-bound, so 4 expensive frames in 90 leave a 16.7ms median
  // untouched. The tail is where a periodic cost shows up, which is what the
  // accessibility mirror's staleness ceiling (`A11Y_MAX_STALE_MS`) is.
  const over = (n) => sorted.filter((x) => x > n).length;
  console.log(`${w}px cols: drawing ${w2.cols}x${w2.rows}, fetched ${w2.colIdx}x${w2.rowIdx} = ${w2.colIdx*w2.rowIdx} cells; frames ${sorted.length}, median ${at(0.5).toFixed(1)}ms p95 ${at(0.95).toFixed(1)}ms max ${sorted[sorted.length-1].toFixed(1)}ms, >20ms ${over(20)} >33ms ${over(33)}`);
}
await b.close();
