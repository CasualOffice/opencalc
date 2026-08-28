// What a person needs when somebody sends them a column.
//
// Asked for by name. The status bar already gives Sum/Avg/Min/Max at a glance;
// this is the other question — how much of it is missing, and *what it is made
// of*. The type distribution is the part a status bar cannot give: it is how
// you find the one text cell wrecking a SUM, or the numbers stored as text that
// make a filter behave oddly.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    // A column as it actually arrives: numbers, a heading, a blank, a number
    // stored as text, and an error.
    const col = ["10", "20", "Total", "", "'0042", "30", "=1/0", "20"];
    col.forEach((v, r) => { if (v !== "") a.session_set_cell(0, r, 0, v); });
  });
  await page.waitForTimeout(300);
}

const panel = (page) => page.locator("#side-panel-body");
const row = (page, label) =>
  page.locator("#side-panel-body .stats-row", { hasText: label }).first().locator(".stats-value");

test("the panel opens from the Data menu and counts what is there", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.column-stats");
  });
  await expect(panel(page)).toBeVisible();
  await expect(row(page, "With a value")).toHaveText("7");
  // A blank is not a zero and is named apart — the commonest question about a
  // column somebody sent you is how much of it is missing.
  await expect(row(page, "Empty")).toHaveText("1");
});

test("it names the numbers stored as text, which is the point", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.column-stats");
  });
  // `'0042` looks like a number and does not add up. A stats panel that
  // silently coerced it would hide the very thing it exists to reveal.
  await expect(row(page, "Numbers stored as text")).toHaveText("1");
  await expect(page.locator("#side-panel-body .stats-row.warn")).toBeVisible();
});

test("the average excludes blanks and text", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.column-stats");
  });
  // 10, 20, 30, 20 — the heading, the blank, the text-number and the error are
  // all excluded. A blank averaging as zero would give 10.
  await expect(row(page, "Average")).toHaveText("20");
  await expect(row(page, "Count").first()).toBeVisible();
});

test("it follows the selection", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 2, "only-one");
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.column-stats");
  });
  await expect(row(page, "With a value")).toHaveText("7");

  // Click the next column and read it — which is how the panel is used.
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 2));
  await expect(row(page, "With a value")).toHaveText("1");
});
