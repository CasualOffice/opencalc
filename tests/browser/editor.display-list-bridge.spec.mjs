// What a display-list frame costs across the WebAssembly boundary.
//
// `RND-10` is deferred on the claim that "a naive per-frame serialisation of a
// display list would be slower than what it replaces". The native half is
// measured by `display-list-frame-serialise` — a median 178 µs against the
// 16.67 ms a 60 fps frame allows. The half that stayed a guess is *this* one:
// the WASM→JS crossing, which nothing could measure because nothing crossed.
//
// This asserts the export is real and its cost is inside a frame. It does not
// assert a specific number: a shared CI runner is not a benchmark rig, and a
// test that pinned microseconds would fail for reasons that have nothing to do
// with the code. The budget is the thing worth defending.

import { expect, test } from "@playwright/test";

const FRAME_MS = 1000 / 60;

async function boot(page) {
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

test("a display list crosses to JS, and does it inside a frame", async ({ page }) => {
  const problems = await boot(page);

  const result = await page.evaluate(() => {
    const wasm = window.opencalcEditor.wasmApi();

    // Fill the viewport first. The seeded document is a handful of cells, and a
    // list of 22 items crosses in less time than `performance.now()` can
    // resolve — which would read as "the bridge is free" while measuring almost
    // nothing. A frame's cost is a frame's worth of content.
    for (let r = 0; r < 60; r += 1) {
      for (let c = 0; c < 26; c += 1) {
        wasm.session_set_cell(0, r, c, r % 3 === 0 ? `Item ${r}-${c}` : String(r * c));
      }
    }
    // A maximised window at 96 dpi.
    const once = () => wasm.session_display_list(0, 1920, 1080, 96);

    once(); // warm: the first call pays for paths nothing has touched yet

    const runs = [];
    for (let i = 0; i < 20; i += 1) {
      const t0 = performance.now();
      const json = once();
      runs.push(performance.now() - t0);
      if (i === 0) var sample = json;
    }
    runs.sort((a, b) => a - b);
    return {
      medianMs: runs[Math.floor(runs.length / 2)],
      worstMs: runs[runs.length - 1],
      bytes: once().length,
      items: JSON.parse(once()).items.length,
    };
  });

  // It has to actually contain the frame, or the timing is of nothing.
  expect(result.items, "the list has paint items in it").toBeGreaterThan(0);

  // The assertion that matters: a whole frame's worth of display list crosses
  // the boundary in less time than a frame has.
  expect(
    result.medianMs,
    `median ${result.medianMs.toFixed(2)}ms for ${result.items} items ` +
      `(${result.bytes} bytes) must fit inside a ${FRAME_MS.toFixed(2)}ms frame`,
  ).toBeLessThan(FRAME_MS);

  console.log(
    `[RND-10] median ${result.medianMs.toFixed(3)}ms  worst ${result.worstMs.toFixed(3)}ms  ` +
      `${result.items} items  ${result.bytes} bytes  (frame budget ${FRAME_MS.toFixed(2)}ms)`,
  );

  expect(problems, "crossing logged nothing").toEqual([]);
});
