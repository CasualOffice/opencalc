// Ribbon Display Options, as in Excel (XL-UX-093).
//
// The chevron at the ribbon's bottom-right switched to the Simplified layout —
// a button whose whole job in Excel is to collapse the ribbon. It now opens
// Excel's Ribbon Display Options (Always show / Show tabs only, and the
// layout), Ctrl+F1 and a double-click on a tab toggle tabs-only, and with
// tabs only a tab shows the ribbon floating over the grid until a command runs.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => { try { localStorage.removeItem("oc.ribbon.display"); localStorage.removeItem("oc.ribbon.layout"); } catch {} });
  await page.reload();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeVisible();
}
const gridTop = (page) => page.locator("#grid-wrap").evaluate((e) => e.getBoundingClientRect().top);

async function chooseDisplay(page, label) {
  await page.locator(".rb-layout").click();
  await page.locator(".rb-pop-item", { hasText: label }).click();
}

test("the chevron opens Ribbon Display Options", async ({ page }) => {
  await boot(page);
  await expect(page.locator(".rb-layout")).toHaveAttribute("aria-label", "Ribbon Display Options");
  await page.locator(".rb-layout").click();
  for (const label of ["Always show Ribbon", "Show tabs only", "Classic ribbon", "Simplified ribbon"]) {
    await expect(page.locator(".rb-pop-item", { hasText: label })).toBeVisible();
  }
  await expect(page.locator('.rb-pop-item[aria-checked="true"]', { hasText: "Always show Ribbon" })).toBeVisible();
});

test("Show tabs only gives the band's height to the grid, and is remembered", async ({ page }) => {
  await boot(page);
  const before = await gridTop(page);
  await chooseDisplay(page, "Show tabs only");
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
  await expect.poll(() => gridTop(page)).toBeLessThan(before - 40);
  await page.reload();
  await expect(page.locator(".oc-ribbon .rb-tabs")).toBeVisible();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
});

test("with tabs only, a tab shows the ribbon over the grid until a command runs", async ({ page }) => {
  await boot(page);
  await chooseDisplay(page, "Show tabs only");
  const top = await gridTop(page);
  await page.locator("#rb-tab-home").click();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeVisible();
  expect(await gridTop(page), "the floating ribbon pushed the grid down").toBe(top);
  await page.locator('.oc-ribbon [data-cmd="toolbar.bold"]').click();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
  await page.locator("#rb-tab-home").click();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
});

test("Ctrl+F1 and a double-click on a tab toggle the ribbon", async ({ page }) => {
  await boot(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+F1");
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
  await page.keyboard.press("Control+F1");
  await expect(page.locator(".oc-ribbon .rb-band")).toBeVisible();
  await page.locator("#rb-tab-home").dblclick();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeHidden();
  await page.locator("#rb-tab-data").dblclick();
  await expect(page.locator(".oc-ribbon .rb-band")).toBeVisible();
  await expect(page.locator("#rb-tab-data")).toHaveAttribute("aria-selected", "true");
});

test("the layout choice moved into the menu still works", async ({ page }) => {
  await boot(page);
  await chooseDisplay(page, "Simplified ribbon");
  await expect(page.locator(".oc-ribbon")).toHaveClass(/rb-simplified/);
  await chooseDisplay(page, "Classic ribbon");
  await expect(page.locator(".oc-ribbon")).not.toHaveClass(/rb-simplified/);
});
