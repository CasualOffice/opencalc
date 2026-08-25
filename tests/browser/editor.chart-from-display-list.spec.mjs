// The canvas draws charts from the engine's display list (`RND-10`).
//
// It used to paint them from its own JavaScript — drawBarChart, drawLineChart,
// drawPie, drawAxes, drawLegend — while the PNG renderer painted the same
// charts from `casual_calc_layout::chart::push_chart`. Two implementations of
// one picture: every fix had to land twice, and a divergence between them was
// invisible until somebody compared a screen to an export.
//
// This asserts what the *item count* could not. Both bugs on the way here
// returned a perfectly plausible list and drew the wrong thing:
//
//   1. The frame was passed in CSS pixels when the layout works in twips
//      (`PX = 15.0`). The plot area came out negative, so `push_chart` drew its
//      background and border and returned — two items, no error, and an empty
//      box where a chart should be.
//   2. Geometry converts by `dpi/1440` and a font size by `dpi/72`. One
//      `ctx.scale` for both would have drawn every label at a fifteenth size.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("pageerror", (e) => problems.push(e.message));
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

test("a chart's bars reach the canvas, not just its frame", async ({ page }) => {
  const problems = await boot(page);

  const result = await page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    for (let i = 0; i < 5; i += 1) {
      api.session_set_cell(0, i, 6, `R${i}`);
      api.session_set_cell(0, i, 7, String((i + 1) * 10));
    }
    api.session_create_chart(0, 0, 6, 4, 7, "column", 8, 1, 16, 12);

    // Twips, which is what the layout works in. This is the conversion that was
    // wrong, so the test states it rather than borrowing it from the caller.
    const T = 1440 / 96;
    const px = (n) => Math.round(n * T);
    const items = JSON.parse(
      api.session_chart_items(0, 0, 0, 0, px(400), px(300)),
    ).items;

    const kinds = items.map((i) => Object.keys(i)[0]);
    return {
      total: items.length,
      polygons: kinds.filter((k) => k === "polygon").length,
      texts: kinds.filter((k) => k === "text").length,
    };
  });

  // A frame alone is two items: background polygon plus border polyline. Five
  // data points must produce more than that, and the bars are polygons.
  expect(result.total, "a chart is more than its frame").toBeGreaterThan(2);
  expect(result.polygons, "the bars themselves").toBeGreaterThan(2);
  expect(result.texts, "axis and category labels").toBeGreaterThan(0);

  expect(problems, "drawing the chart logged nothing").toEqual([]);
});

test("the chart reaches the canvas through the editor's own draw path", async ({ page }) => {
  await boot(page);

  // Through `drawCharts`, not by calling the export directly.
  //
  // The first version of this test built its own frame in twips and asked the
  // export for items — and it passed with the *caller* reverted to pixels,
  // because it never used the caller. A test that supplies the very conversion
  // under test proves only that the export works when handed correct input.
  //
  // So this looks at the pixels: it renders, then counts how much of the
  // chart's area is bar-coloured. An empty frame is white; a drawn chart is
  // not.
  const painted = await page.evaluate(async () => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    for (let i = 0; i < 5; i += 1) {
      api.session_set_cell(0, i, 6, `R${i}`);
      api.session_set_cell(0, i, 7, String((i + 1) * 10));
    }
    api.session_create_chart(0, 0, 6, 4, 7, "column", 8, 1, 16, 12);
    ed.draw();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

    const canvas = document.querySelector("#grid");
    const ctx = canvas.getContext("2d");
    const frame = ed.chartFrames?.[0];
    if (!frame) return { error: "no chart frame" };
    const dpr = window.devicePixelRatio || 1;
    const d = ctx.getImageData(frame.x * dpr, frame.y * dpr, frame.w * dpr, frame.h * dpr).data;

    let coloured = 0;
    for (let i = 0; i < d.length; i += 4) {
      const [r, g, b] = [d[i], d[i + 1], d[i + 2]];
      // Anything meaningfully not white or grey: the bars are blue.
      if (b - r > 30 && d[i + 3] > 0) coloured += 1;
    }
    return { coloured, total: d.length / 4 };
  });

  expect(painted.error, "the chart has a frame to sample").toBeUndefined();
  // Five bars across a 400×300 frame cover a good fraction of it. An empty box
  // scores zero, which is what the pixel-versus-twip bug produced.
  expect(
    painted.coloured,
    `only ${painted.coloured} of ${painted.total} pixels are bar-coloured — the chart drew as an empty frame`,
  ).toBeGreaterThan(500);
});
