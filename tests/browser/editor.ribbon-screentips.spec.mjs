// Ribbon ScreenTips, as in Excel (XL-UX-008).
//
// Excel does not use the operating system's tooltip on the ribbon. Resting the
// pointer on a control for about half a second opens a ScreenTip: the
// command's name and shortcut in bold, then one line saying what it does. It
// also opens when the control is reached from the keyboard, and it goes away
// on mouse-out, Escape or a click. The ribbon had only the native `title`
// bubble — a second-long wait, the browser's own styling, a name with no
// description — and nothing at all for a keyboard user.
//
// The native `title` has to go as well as the ScreenTip arriving: with both,
// two tips show at once, one on top of the other.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await expect(page.locator(".oc-ribbon .rb-panel:not([hidden])")).toBeVisible();
}

const tip = (page) => page.locator(".rb-screentip");
const bold = (page) => page.locator('.oc-ribbon .rb-panel:not([hidden]) [data-cmd="toolbar.bold"]');

test("resting on a ribbon button opens a ScreenTip with its name, shortcut and what it does", async ({ page }) => {
  await boot(page);
  await bold(page).hover();
  // Not instantly: a pointer crossing the ribbon must not flash a tip on every
  // button it passes over.
  await page.waitForTimeout(150);
  // Read once, not retried: a retrying assertion would wait out the delay.
  expect(await tip(page).isVisible(), "a ScreenTip opened before the hover delay").toBe(false);
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await expect(tip(page).locator(".rb-screentip-title")).toHaveText("Bold (Ctrl+B)");
  await expect(tip(page).locator(".rb-screentip-desc")).toHaveText("Make your text bold.");
  const weight = await tip(page).locator(".rb-screentip-title").evaluate((n) => Number(getComputedStyle(n).fontWeight));
  expect(weight, "the title is bold, as in Excel").toBeGreaterThanOrEqual(600);
  // Accessible: the description is the control's description while it shows.
  const id = await tip(page).getAttribute("id");
  await expect(bold(page)).toHaveAttribute("aria-describedby", id);
});

test("no ribbon or Quick Access Toolbar control carries a native title", async ({ page }) => {
  await boot(page);
  // Two tips at once — the browser's bubble under the ScreenTip — is what a
  // stray `title` produces.
  const titled = await page.evaluate(() =>
    [...document.querySelectorAll(".oc-ribbon [title], .rb-qat [title]")].map((n) => n.dataset.cmd || n.className));
  expect(titled).toEqual([]);
});

test("the ScreenTip closes on mouse-out, on Escape and on a click", async ({ page }) => {
  await boot(page);
  const italic = page.locator('.oc-ribbon .rb-panel:not([hidden]) [data-cmd="toolbar.italic"]');

  await italic.hover();
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await page.mouse.move(5, 600);
  await expect(tip(page), "mouse-out left it open").toBeHidden();

  await italic.hover();
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await page.keyboard.press("Escape");
  await expect(tip(page), "Escape left it open").toBeHidden();

  await page.mouse.move(5, 600);
  await italic.hover();
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await italic.click();
  await expect(tip(page), "a click left it open").toBeHidden();
});

test("a keyboard user reaching a ribbon button gets its ScreenTip", async ({ page }) => {
  await boot(page);
  // Focus arriving from the keyboard (`:focus-visible`), not a click.
  await page.keyboard.press("Shift");
  await bold(page).focus();
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await expect(tip(page).locator(".rb-screentip-title")).toHaveText("Bold (Ctrl+B)");
  await page.keyboard.press("Escape");
  await expect(tip(page)).toBeHidden();
});

test("the Quick Access Toolbar has ScreenTips too", async ({ page }) => {
  await boot(page);
  await page.locator('.rb-qat [data-oc-proxy="edit.undo"]').hover();
  await expect(tip(page)).toBeVisible({ timeout: 1500 });
  await expect(tip(page).locator(".rb-screentip-title")).toHaveText(/^Undo/);
  await expect(tip(page).locator(".rb-screentip-desc")).toHaveText("Undo your last action.");
});

test("every control on every tab has a description to show", async ({ page }) => {
  await boot(page);
  // Read from the controls the ribbon drew, so a control added without a
  // description is caught whichever tab it is on.
  const missing = await page.evaluate(() =>
    [...document.querySelectorAll(".oc-ribbon .rb-panel [data-cmd]")]
      .filter((n) => !n.dataset.tipDesc)   // hidden tabs' panels included
      .map((n) => n.dataset.cmd));
  expect(missing, "ribbon controls with no ScreenTip description").toEqual([]);
});

test("clicking into the Font box does not open its ScreenTip", async ({ page }) => {
  await boot(page);
  // A text box matches `:focus-visible` even when clicked, so focus alone is
  // not proof the keyboard put it there: the click that focused it has just
  // put the tip away, and it stays away until the pointer leaves.
  await page.locator('.oc-ribbon .rb-panel:not([hidden]) input[data-cmd="toolbar.font"]').click();
  await page.waitForTimeout(800);
  expect(await tip(page).isVisible(), "a click into the box opened its ScreenTip").toBe(false);
});
