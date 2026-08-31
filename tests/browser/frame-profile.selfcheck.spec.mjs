// The profiler measures a cost whose size is already known.
//
// `PERF-13`. `frame-profile.mjs` reported **the same 16.6ms with 1,963,431
// bytes crossing the WASM boundary every frame as with 61**, so an 8.80ms
// per-frame cost was invisible to this repository's own frame profiler. Two
// structural reasons, and this file gates both:
//
//   - it timed the *interval* between animation frames, which the compositor
//     paces to the display period, so anything that fits under vsync is
//     absorbed; and
//   - it seeded no charts at all, so it could not have exercised the path in
//     principle.
//
// A profiler that has never been shown to detect a known cost is not a
// profiler, and an unexercised path is not a case. Both are testable without
// asserting any performance budget, which matters: `editor.frame-budget.spec.mjs`
// says why a wall-clock assertion on a shared machine is a flaky test that gets
// deleted, and that reasoning is right.
//
// **Neither test here is a budget.** The first injects a busy-wait of a known
// length and asserts the timing path *attributes* it — a self-consistency
// check, where the expected value is defined by the test rather than by the
// machine, and where noise can only push the measurement further past the
// threshold. The second asserts the chart case builds the chart it claims to.
// Neither can fail because a runner was slow; the first fails if the timing
// path stops being per-call, and the second if the seed stops seeding.
//
// It imports the profiler's own functions rather than restating them. A gate
// that keeps a private copy of the thing it is gating is the door-and-probe
// failure this row is an instance of.

import { expect, test } from "@playwright/test";
import { measureCalls, seedCharts, stats } from "./frame-profile.mjs";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 60; r += 1) {
      for (let c = 0; c < 12; c += 1) a.session_set_cell(0, r, c, `r${r}c${c}`);
    }
  });
  await page.evaluate(() => new Promise((res) => {
    window.opencalcEditor.draw();
    requestAnimationFrame(() => requestAnimationFrame(res));
  }));
}

const BURN_MS = 8;
/// Half the burn, in both directions. Nothing between 4ms and 8ms is a number
/// either implementation produces, so the band is wide enough that no runner
/// lands in it and narrow enough that neither answer can be mistaken for the
/// other.
const HALF = BURN_MS / 2;

const median = async (page, probe, burnMs) => stats(
  (await page.evaluate(measureCalls, { probe, reps: 20, warmup: 5, burnMs })).ms,
).p50;

/// The display period, measured on the machine rather than assumed.
///
/// `PERF-13`'s signature is that the report **equals** this number, so this is
/// the thing a probe has to be distinguishable *from*. Bounding the draw probe
/// against `HALF` instead was a wall-clock budget in disguise — the one thing
/// the header of this file promises it does not contain — and it failed on
/// main at `4.10ms`, drawing 720 cells on a contended runner.
const displayPeriod = (page) => page.evaluate(() => new Promise((res) => {
  const ts = [];
  const tick = (t) => {
    ts.push(t);
    if (ts.length < 12) { requestAnimationFrame(tick); return; }
    const d = ts.slice(1).map((v, i) => v - ts[i]).sort((a, b) => a - b);
    res(d[d.length >> 1]);
  };
  requestAnimationFrame(tick);
}));

test("the call timing mode attributes a cost of a size the test chose", async ({ page }) => {
  await boot(page);

  // **The median, and deliberately not the minimum.** The minimum is the right
  // statistic for *reading* the profiler — noise is one-sided, so it is the
  // number that moves only when the work does. It is the wrong statistic for
  // this test: an interval measurement leaks a cost through its first, partial
  // interval, and the mutation that reverts this file to `PERF-13`'s behaviour
  // reported `min 5.2ms` quiet against `min 12.2ms` burdened — it would have
  // passed a minimum-based assertion while its median sat at 16.5ms against
  // 16.7ms, which is the defect exactly. The median is what the vsync floor
  // pins, so the median is what has to be checked.
  const idleQuiet = await median(page, "idle", 0);
  const idleBurdened = await median(page, "idle", BURN_MS);
  const drawQuiet = await median(page, "draw", 0);

  // **Not floored at the display period.** An empty probe costs nothing; an
  // interval-based measurement of it reports the refresh rate, ~16.7ms, and
  // that is the whole of `PERF-13` in one number. This is not a performance
  // budget — the probe does no work, so there is nothing for a slow machine to
  // be slow at.
  expect(
    idleQuiet,
    `an empty probe measured ${idleQuiet.toFixed(2)}ms. A probe that does nothing costs `
    + `nothing; anything near the display period means the interval is being reported `
    + `instead of the work, which is what PERF-13 is.`,
  ).toBeLessThan(HALF);
  // **Against the display period, not against `HALF`.** `idleQuiet` above can
  // use a fixed bound because an empty probe costs nothing anywhere. This probe
  // draws 720 cells, so its cost is a property of the machine, and any constant
  // ceiling is a performance budget on a shared runner — which is the flaky
  // test this file's header promises not to be, and which it became at 4.10ms
  // against a 4ms bound. What `PERF-13` looks like is the report *equalling*
  // the period, so that is what has to be excluded.
  const period = await displayPeriod(page);
  const drawCeiling = Math.max(period * 0.75, HALF);
  expect(
    drawQuiet,
    `a draw of a 60x12 sheet measured ${drawQuiet.toFixed(2)}ms against a display period of `
    + `${period.toFixed(2)}ms, at or past ${drawCeiling.toFixed(2)}ms — the interval is being `
    + `reported instead of the work, which is what PERF-13 is.`,
  ).toBeLessThan(drawCeiling);

  // **And it does see a cost that is there.** The busy-wait is at least
  // `BURN_MS` by construction, so noise can only push this further past the
  // threshold.
  expect(
    idleBurdened,
    `${BURN_MS}ms was injected into the timed call and the profiler reported `
    + `${idleBurdened.toFixed(2)}ms. A profiler that cannot detect a known cost reports `
    + `numbers that mean nothing.`,
  ).toBeGreaterThanOrEqual(HALF);
});

test("the chart case seeds the chart it reports on", async ({ page }) => {
  await boot(page);

  const plain = await seedCharts(page, "rows");
  expect(plain.charts, "the `rows` seed built no chart, so its numbers are a blank sheet's")
    .toBe(1);
  expect(plain.refs.length, "the chart has no series, so nothing is resolved for it").toBe(6);

  await boot(page);
  const pathological = await seedCharts(page, "columns");
  expect(pathological.charts, "the `columns` seed built no chart").toBe(1);
  // The case `CHT-13` found: a chart whose series name whole columns on cells
  // that hold nothing. Reached by opening a file, not by having a big sheet.
  expect(
    pathological.refs.join(),
    "the `columns` seed did not widen its series, so it is the `rows` case again",
  ).toMatch(/1048576/);

  // And the probe reads something back for it. `chartFrames` returning `"[]"`
  // would make every chart number in the report a measurement of nothing.
  const r = await page.evaluate(measureCalls, {
    probe: "chartFrames", reps: 5, warmup: 1, burnMs: 0,
  });
  expect(r.bytes, "the chart payload probe came back empty").toBeGreaterThan(2);
});
