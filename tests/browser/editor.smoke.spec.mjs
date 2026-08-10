// The `browser-smoke` gate: does the editor load, paint, and calculate?
//
// This is the one thing no Rust test can tell us. The engine's correctness is
// gated by 600-odd tests in `crates/`, and none of them would notice if the
// WebAssembly glue failed to instantiate, the canvas never painted, or the
// bindings and the engine disagreed about a signature — which has happened:
// a stale `pkg/` once shipped 25 of 222 exports and every Rust gate stayed
// green. AGENTS.md requires verifying in a browser before calling anything
// done; until now that was done by hand, every time.
//
// # What it asserts through
//
// Real user surfaces only — the name box, the formula bar, and the
// accessibility mirror the editor maintains of the visible cells. No hooks
// added for testing. If the mirror is wrong the test is right to fail: it is
// what a screen reader reads.

import { expect, test } from "@playwright/test";

/// The seeded document `editor.js` opens with, as cell coordinates. Zero-based
/// row/column, because that is what the accessibility mirror's ids use.
const D2 = { row: 1, col: 3 }; // =B2*C2 → 3 × 4.50
const D5 = { row: 4, col: 3 }; // =SUM(D2:D4)

/// The text the accessibility mirror shows for a cell — what a screen reader
/// would read, and the only structural view of the grid that exists, since the
/// cells themselves are pixels on a canvas.
function cell(page, { row, col }) {
  return page.locator(`#a11y-${row}-${col}`);
}

/// Load the editor and wait for the engine, failing loudly on anything the
/// page logged on the way.
///
/// The status line is the editor's own readiness signal: `main()` sets it to
/// the engine version last of all, and to `failed: …` if any step threw. That
/// makes it a boot assertion rather than a sleep.
async function boot(page) {
  const problems = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") problems.push(`console: ${msg.text()}`);
  });
  page.on("pageerror", (err) => problems.push(`uncaught: ${err.message}`));

  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, {
    timeout: 30_000,
  });
  expect(problems, "the page logged nothing while booting").toEqual([]);
  return problems;
}

/// Move the selection with the name box, as a user does.
async function goTo(page, ref) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
}

/// Commit a value or formula into the selected cell via the formula bar.
async function type(page, text) {
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}

test("the editor boots, reports its engine version, and logs nothing", async ({
  page,
}) => {
  await boot(page);
  // The version is the engine's own, so a page served beside a `pkg/` from a
  // different build says so rather than running it.
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d+\.\d+\.\d+$/);
});

test("the grid canvas actually paints", async ({ page }) => {
  await boot(page);

  // A canvas that never painted is transparent, and one that painted only its
  // white ground has no gridlines — either way the editor looks like it
  // loaded, which is exactly the failure a status line cannot catch.
  const painted = await page.evaluate(() => {
    const canvas = document.getElementById("grid");
    if (!canvas || !canvas.width || !canvas.height) return null;
    const ctx = canvas.getContext("2d");
    const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
    let opaque = 0;
    let ink = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i + 3] < 250) continue;
      opaque += 1;
      // Anything that is not the ground: a gridline, a header, a glyph.
      if (data[i] < 240 || data[i + 1] < 240 || data[i + 2] < 240) ink += 1;
    }
    return { width: canvas.width, height: canvas.height, opaque, ink };
  });

  expect(painted, "there is a sized #grid canvas").not.toBeNull();
  expect(painted.opaque, "the canvas is opaque, so something drew on it").toBeGreaterThan(
    painted.width * painted.height * 0.9,
  );
  expect(painted.ink, "and it has content, not just a white ground").toBeGreaterThan(1000);
});

test("the engine calculates: the seeded formulas have values", async ({ page }) => {
  await boot(page);

  // `=B2*C2` over 3 and 4.50, and `=SUM(D2:D4)` over that and two more. These
  // are numbers only a working evaluator produces; a broken binding shows the
  // formula text, an empty cell, or an error.
  await expect(cell(page, D2)).toHaveText("13.5");
  await expect(cell(page, D5)).toHaveText("43.48");
});

test("an edit recalculates its dependents", async ({ page }) => {
  await boot(page);

  await goTo(page, "B2");
  await expect(page.locator("#cell-ref")).toHaveValue("B2");
  await type(page, "10");

  // Both the direct dependent and the one behind it: a recalc that stops at
  // the first level is a real bug and looks like success at D2 alone.
  await expect(cell(page, D2), "=B2*C2 followed the edit").toHaveText("45");
  await expect(cell(page, D5), "and =SUM(D2:D4) followed D2").toHaveText("74.98");
});

test("one undo reverses one edit", async ({ page }) => {
  // This is why the gate exists. It failed on the first run: every commit also
  // submitted a sheet-metadata bundle that differed in nothing, and that no-op
  // went on the undo stack, so the first Ctrl+Z popped a phantom and appeared
  // to do nothing at all. A user who presses undo and sees no change does not
  // press it again; they conclude undo is broken, which it was.
  await boot(page);

  await goTo(page, "B2");
  await type(page, "10");
  await expect(cell(page, D2)).toHaveText("45");

  await page.click("#grid");
  await page.keyboard.press("ControlOrMeta+z");
  await expect(cell(page, D2), "one press, one edit reversed").toHaveText("13.5");
  await expect(cell(page, D5)).toHaveText("43.48");

  // And by the toolbar, which is the same command by another route.
  await page.click("#tb-redo");
  await expect(cell(page, D2)).toHaveText("45");
  await page.click("#tb-undo");
  await expect(cell(page, D2)).toHaveText("13.5");
});

test("the freshly opened document has nothing to undo", async ({ page }) => {
  // The editor seeds its sheet through the same edit path a user types on, so
  // without an explicit starting point Ctrl+Z takes the document apart cell by
  // cell and leaves an empty grid.
  await boot(page);
  await expect(page.locator("#tb-undo")).toBeDisabled();
  await expect(page.locator("#tb-redo")).toBeDisabled();

  await goTo(page, "B2");
  await type(page, "10");
  await expect(page.locator("#tb-undo"), "and enables once there is one").toBeEnabled();
});

test("the undo tooltip names the edit it would reverse", async ({ page }) => {
  await boot(page);
  const undo = page.locator("#tb-undo");

  await goTo(page, "B2");
  await type(page, "10");

  // Read from `data-tip`, which is what the tooltip layer displays: it moves
  // `title` onto that attribute and deletes it at boot, so code that later
  // assigned `title` updated something nobody sees — and brought back the
  // native bubble that tipify exists to suppress.
  await expect(undo).toHaveAttribute("data-tip", /^Undo cell edit/);
  await expect(undo, "the native bubble stays suppressed").not.toHaveAttribute("title", /./);
  await expect(undo).toHaveAttribute("aria-label", /^Undo cell edit/);
});

test("a formula comes back written the way it was typed", async ({ page }) => {
  await boot(page);

  await goTo(page, "F1");
  await type(page, "=1+2*3");
  await expect(cell(page, { row: 0, col: 5 }), "precedence, not left to right").toHaveText(
    "7",
  );

  // A cell does not store the text that was typed — it stores the parsed tree,
  // and the formula bar prints it back. So the printer *is* the formula as far
  // as anyone editing it is concerned, and it used to bracket every operator:
  // this came back as `=(1+(2*3))`, which is the same formula and not the one
  // the user wrote. It reaches the saved file too.
  await goTo(page, "F1");
  await expect(page.locator("#formula-input")).toHaveValue("=1+2*3");

  // Brackets the grammar does need are kept.
  await goTo(page, "F2");
  await type(page, "=(1+2)*3");
  await goTo(page, "F2");
  await expect(page.locator("#formula-input")).toHaveValue("=(1+2)*3");
});
