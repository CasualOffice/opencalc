// A workbook is untrusted input.
//
// Anybody can send one and opening it is the point of the product, so any path
// that turns workbook text into markup runs script in the editor's origin —
// where the document, the session token and the collaboration socket live.
// SEC-001 in docs/67.
//
// The fixture carries the same payload in a defined name, its `refersTo`, a
// sheet name and a cell value. Refusing hostile *text* is not the defence and
// is not attempted: not building DOM out of it is.

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const HOSTILE = new URL("../../fixtures/generated/hostile-names.xlsx", import.meta.url);

async function openHostile(page) {
  const problems = [];
  const requests = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  page.on("request", (r) => requests.push(r.url()));

  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  // Loaded through the engine, which is the path a real open takes.
  await page.evaluate(async (bytes) => {
    window.opencalcEditor.wasmApi().session_open(new Uint8Array(bytes));
    window.opencalcEditor.relayout();
  }, [...readFileSync(HOSTILE)]);

  return { problems, requests };
}

/// Nothing the workbook said became an element or an attribute.
async function noInjection(page) {
  return page.evaluate(() => ({
    pwned: window.__pwned ?? null,
    images: document.querySelectorAll("img").length,
    scripts: document.querySelectorAll("script[data-from-workbook], body script").length,
    // Any element carrying an inline handler at all. The payload's whole
    // purpose is to become one of these.
    handlers: [...document.querySelectorAll("*")].filter((el) =>
      [...el.attributes].some((a) => a.name.toLowerCase().startsWith("on")),
    ).length,
  }));
}

test("a hostile defined name cannot create DOM in the Name Manager", async ({ page }) => {
  const { problems, requests } = await openHostile(page);

  // The Name Manager is the sink the finding named: it built its rows with
  // innerHTML out of the name and its target.
  await page.locator("#grid").focus();
  await page.keyboard.press("ControlOrMeta+F3");
  await page.waitForTimeout(300);

  const seen = await noInjection(page);
  expect(seen.pwned, "no script ran").toBeNull();
  expect(seen.images, "the payload did not become an <img>").toBe(0);
  expect(seen.handlers, "nothing acquired an inline event handler").toBe(0);
  expect(
    requests.filter((u) => /\/x(\?|$)|onerror/.test(u)),
    "nothing in the workbook was fetched",
  ).toEqual([]);

  // And it is still *shown* — escaping into oblivion would be a different bug.
  const shown = await page.evaluate(() =>
    [...document.querySelectorAll(".nm-row")].map((r) => r.textContent).join(" "),
  );
  expect(shown, "the name is displayed as text").toContain("<img");
  expect(problems).toEqual([]);
});

test("a hostile sheet name and cell value cannot create DOM", async ({ page }) => {
  const { problems } = await openHostile(page);

  // The tab strip, the formula bar and the status line all show workbook text.
  await page.locator("#cell-ref").fill("A1");
  await page.locator("#cell-ref").press("Enter");
  await page.waitForTimeout(200);

  const seen = await noInjection(page);
  expect(seen.pwned, "no script ran").toBeNull();
  expect(seen.images).toBe(0);
  expect(seen.handlers).toBe(0);

  expect(
    await page.locator("#formula-input").inputValue(),
    "the cell's text is shown as text",
  ).toContain("<img");
  expect(problems).toEqual([]);
});
