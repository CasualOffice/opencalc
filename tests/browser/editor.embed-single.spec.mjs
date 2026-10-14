// A second <opencalc-sheet> on a page refuses instead of taking over the first.
//
// Until several embeds can each have their own editor (UX-SHELL-24), two
// elements share one editor core, and the second silently rebound it: the
// first element's handle then read — and its save() would have written — the
// second element's empty workbook. That is data loss with no signal. The
// second element now refuses with a named error and says so on screen; the
// first keeps its workbook (EMB-GUARD, docs/100 step 1).

import { expect, test } from "@playwright/test";

test("a second embed refuses, and the first keeps its workbook", async ({ page }) => {
  await page.route("**/__embed-single.html", (route) => route.fulfill({
    contentType: "text/html",
    body: `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0">
      <opencalc-sheet id="a" style="display:block;height:360px;width:900px"></opencalc-sheet>
      <script type="module">import "/embed.js";</script></body></html>`,
  }));
  await page.goto("/__embed-single.html");
  await page.waitForFunction(() => document.getElementById("a")?.ready?.then, null, { timeout: 30_000 });
  await page.evaluate(async () => {
    const ed = await document.getElementById("a").ready;
    ed.wasmApi().session_set_cell(0, 0, 0, "from A");
  });

  const second = await page.evaluate(async () => {
    const b = document.createElement("opencalc-sheet");
    b.id = "b";
    b.style.cssText = "display:block;height:200px;width:600px";
    document.body.appendChild(b);
    try { await b.ready; return { refused: false }; }
    catch (e) { return { refused: true, name: e.name, message: e.message }; }
  });
  expect(second.refused, "a second embed started and took over the first's editor").toBe(true);
  expect(second.name).toBe("OpenCalcEmbedLimitError");
  await expect(page.locator("opencalc-sheet#b").getByText(/one OpenCalc sheet per page/i)).toBeVisible();

  const a1 = await page.evaluate(async () => (await document.getElementById("a").ready).wasmApi().session_cell_input(0, 0, 0));
  expect(a1, "the first embed's workbook was replaced").toBe("from A");
});

test("a sheet can mount again once the first is removed", async ({ page }) => {
  await page.route("**/__embed-single2.html", (route) => route.fulfill({
    contentType: "text/html",
    body: `<!doctype html><html><head><meta charset="utf-8"></head><body style="margin:0">
      <opencalc-sheet id="a" style="display:block;height:360px;width:900px"></opencalc-sheet>
      <script type="module">import "/embed.js";</script></body></html>`,
  }));
  await page.goto("/__embed-single2.html");
  await page.waitForFunction(() => document.getElementById("a")?.ready?.then, null, { timeout: 30_000 });
  await page.evaluate(() => document.getElementById("a").ready);
  const ok = await page.evaluate(async () => {
    document.getElementById("a").remove();
    const b = document.createElement("opencalc-sheet");
    b.style.cssText = "display:block;height:300px;width:800px";
    document.body.appendChild(b);
    try { await b.ready; return true; } catch { return false; }
  });
  expect(ok, "a replacement embed was refused although the first was gone").toBe(true);
});
