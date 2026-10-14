// `<opencalc-sheet>` boots from an installed copy of `@opencalc/sheet` (SDK-012).
//
// The page is `fixture.mjs`'s: a static app whose only OpenCalc code is what
// `npm install` put in `node_modules`. So every file the element reaches for —
// the editor's topic modules, the shell, the icon sprite, the fonts, the wasm
// and its glue, and the modules it loads only on demand — either came out of
// the tarball or is a 404 here.

import { expect, test } from "@playwright/test";

/// Every request the page made that did not succeed, and every uncaught error.
function watch(page) {
  const failures = [];
  page.on("response", (r) => {
    if (r.status() >= 400) failures.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });
  page.on("requestfailed", (r) => failures.push(`failed ${new URL(r.url()).pathname}: ${r.failure()?.errorText}`));
  page.on("pageerror", (e) => failures.push(`pageerror: ${e.message}`));
  return failures;
}

test("the installed element boots the engine and draws the ribbon", async ({ page }) => {
  const failures = watch(page);
  await page.goto("/index.html");
  const s = page.locator("opencalc-sheet#sheet");

  // The engine: the status bar carries the engine's version once the wasm is
  // instantiated and a session is open.
  // Polled together with the failures, so a red run names the missing files
  // rather than only saying the status bar never changed.
  await expect.poll(async () => {
    if (failures.length) return failures.join(" | ");
    return (await s.locator("#tb-status").getAttribute("data-engine")) ?? "not booted yet";
  }, { message: "the installed element did not boot its engine", timeout: 30_000 })
    .toMatch(/^\d+\.\d+\.\d+/);
  await page.evaluate(() => document.getElementById("sheet").ready);

  // The shell: the ribbon's tab row and band, with an icon actually drawn
  // from the sprite the package ships.
  await expect(s.locator(".oc-ribbon .rb-tabs")).toBeVisible();
  await expect(s.locator(".oc-ribbon .rb-band")).toBeVisible();
  await expect(s.locator("#rb-tab-home")).toBeVisible();
  const iconDrawn = await page.evaluate(() => {
    const root = document.getElementById("sheet").shadowRoot;
    const use = root.querySelector('.rb-band [data-cmd="toolbar.bold"] use');
    const id = use?.getAttribute("href")?.slice(1);
    const box = use?.getBBox();
    return !!(id && root.getElementById(id) && box && box.width > 0 && box.height > 0);
  });
  expect(iconDrawn, "the Bold icon did not draw from the packaged sprite").toBe(true);

  // A round trip through the engine, so "booted" means "computes".
  const value = await page.evaluate(async () => {
    const ed = await document.getElementById("sheet").ready;
    const api = ed.wasmApi();
    api.session_set_cell(0, 0, 0, "=6*7");
    return JSON.parse(api.session_cells(0, 0, 0, 0, 0))[0]?.t;
  }).catch((e) => `error: ${e.message}`);
  expect(value).toBe("42");

  // A module loaded only on demand: Insert Function pulls in `fn-info.js`.
  await page.evaluate(() => document.getElementById("sheet").run("formulas.insert-function"));
  await expect(s.locator("#oc-modal-title")).toHaveText("Insert Function");
  await expect(s.locator("#oc-fn-list .fn-row").first()).toHaveText("SUM");

  // Fonts load only when something draws in them, so a missing face would go
  // unnoticed by a sheet with no text in it. Load every face the element
  // declared — the list comes from its own stylesheets, not from this test.
  const fonts = await page.evaluate(async () => {
    const faces = [...document.fonts];
    const broken = [];
    await Promise.all(faces.map((f) => f.load().catch(() => broken.push(`${f.family} ${f.weight} ${f.style}`))));
    return { declared: faces.length, broken };
  });
  expect(fonts.declared, "the element declared no font faces").toBeGreaterThan(0);
  expect(fonts.broken, "font faces the installed package could not supply").toEqual([]);

  expect(failures, "requests the installed package could not satisfy").toEqual([]);
});

test("every module in the installed package loads, including the ones loaded on demand", async ({ page }) => {
  // Collaboration's `collab.js` is only imported when a document is shared,
  // which this page never does. Importing every module the package ships
  // proves each one's own imports resolve inside the package; the walk in
  // `build-packages.mjs` is what decides that they are shipped at all.
  const failures = watch(page);
  await page.goto("/index.html");
  await page.evaluate(() => document.getElementById("sheet").ready);
  const modules = await (await page.request.get("/__installed")).json();
  expect(modules, "the package ships no modules").toContain("collab.js");
  const errors = await page.evaluate(async (names) => {
    const out = [];
    for (const name of names) {
      try {
        await import(`/node_modules/@opencalc/sheet/dist/${name}`);
      } catch (e) {
        out.push(`${name}: ${e.message}`);
      }
    }
    return out;
  }, modules);
  expect(errors).toEqual([]);
  expect(failures).toEqual([]);
});
