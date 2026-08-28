import { chromium } from "@playwright/test";
const b = await chromium.launch();

async function measure(rows) {
  const p = await (await b.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  await p.goto("http://127.0.0.1:8123/editor.html", { waitUntil: "networkidle" });
  await p.waitForFunction(() => document.querySelector("#tb-status")?.textContent?.startsWith("engine v"), null, { timeout: 30000 });
  await p.evaluate((rows) => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < rows; r++)
      for (let c = 0; c < 12; c++)
        a.session_set_cell(0, r, c, c % 3 === 0 ? `Item ${r}-${c}` : String((r * 7 + c) % 997));
  }, rows);
  await p.waitForTimeout(800);
  // Time draw() itself, away from vsync, so the number is the work and not the clock.
  const ms = await p.evaluate(async () => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 0);
    const t0 = performance.now();
    const N = 30;
    for (let i = 0; i < N; i++) {
      ed.wasmApi().session_set_cell(0, 0, 20, String(i)); // force a real redraw
      ed.drawForTest ? ed.drawForTest() : null;
      await new Promise((r) => requestAnimationFrame(r));
    }
    return (performance.now() - t0) / N;
  });
  await p.close();
  return +ms.toFixed(2);
}

for (const rows of [200, 4000, 20000]) {
  console.log(`${String(rows).padStart(6)} rows of content -> ${await measure(rows)} ms per frame`);
}
await b.close();
