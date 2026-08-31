// Who last changed this cell (`HIST-02`, `docs/89`).
//
// The model carried no authorship at all — an `Operation` has no author, and
// `undo_would_discard` relied on the absence. This is the visible end of
// closing that: a hover that names the person, and says what a save will lose.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

const authorOf = (page, r, c) => page.evaluate(([row, col]) =>
  window.opencalcEditor.wasmApi().session_cell_author(0, row, col), [r, c]);

/// **A local session attributes nothing**, which is what keeps it free.
test("an unshared session names nobody", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "hello"));
  await page.waitForTimeout(200);
  expect(await authorOf(page, 0, 0),
    "a local session recorded an author, so every unshared workbook pays for one").toBe("");
});

test("an edit made after the host names the author is attributed to them", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_author("u_7", "Priya Sharma");
    a.session_set_cell(0, 2, 1, "42");
  });
  await page.waitForTimeout(250);
  expect(await authorOf(page, 2, 1)).toBe("Priya Sharma");
  // And a cell nobody touched is still nobody's.
  expect(await authorOf(page, 5, 5)).toBe("");
});

/// **The hover says who, and what a save will lose.**
///
/// The second half is not a hedge. Attribution does not survive a round-trip,
/// and unlike an image that visibly vanishes it *looks* present in a reopened
/// file — the cells are all still there. The correction has to sit next to the
/// claim.
test("hovering an attributed cell names the person and the limit", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_author("u_7", "Priya Sharma");
    a.session_set_cell(0, 0, 0, "42");
  });
  await page.waitForTimeout(300);

  const box = await page.evaluate(() => {
    const b = window.opencalcEditor.cellBoxForTest(0, 0);
    const c = document.querySelector("canvas").getBoundingClientRect();
    return { x: c.x + b.x + b.w / 2, y: c.y + b.y + b.h / 2 };
  });
  await page.mouse.move(box.x, box.y);
  await page.waitForTimeout(300);

  const tip = page.locator("#comment-tip, .comment-tip").first();
  const text = await page.evaluate(() => {
    const el = [...document.querySelectorAll("div,span")]
      .find((e) => !e.hidden && /Last changed by/.test(e.textContent || ""));
    return el ? el.textContent.replace(/\s+/g, " ").trim() : "";
  });
  expect(text, "hovering an attributed cell said nothing").toContain("Priya Sharma");
  expect(text, "the hover does not say the attribution is lost on save").toContain("since this file was opened");
});

/// **Clearing the author stops the stamping**, rather than leaving a stale name
/// on everything typed afterwards.
test("clearing the author stops attributing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_author("u_7", "Priya Sharma");
    a.session_set_cell(0, 0, 0, "one");
    a.session_set_author("", "");
    a.session_set_cell(0, 1, 0, "two");
  });
  await page.waitForTimeout(300);
  expect(await authorOf(page, 0, 0)).toBe("Priya Sharma");
  expect(await authorOf(page, 1, 0),
    "an edit made after the author was cleared still carries their name").toBe("");
});
