// No control the chrome offers may be unreachable — at any width.
//
// Retaken for the ribbon shell (UX-SHELL-21). This file was written against the
// menu bar and toolbar, which the ribbon parks off-screen; its width checks went
// on passing there, measuring a toolbar nobody could see, while the two tests
// that drove its overflow buttons failed. What it protects is unchanged.
//
// The history that made it: the old toolbar folded its groups well, but had no
// answer for running out of groups — fully collapsed it still measured 791px,
// so every viewport narrower than that clipped it with nothing left to fold.
// A clipped control is not a broken one; it renders perfectly, right up to the
// edge of a viewport nobody tested at.
//
// So this asserts the invariant rather than a layout: at every width down to
// 320px, nothing the shell draws runs past the window, every tab is reachable,
// and every command on a tab is either on screen or behind something that
// opens on screen. Above 600px that is the ribbon, folding its groups; at 600
// and below it is the phone's bottom bar.

import { expect, test } from "@playwright/test";

// 320 is the narrowest phone still sold; 640 is just past the phone
// breakpoint, where the ribbon has the least room it will ever get; 768 is an
// iPad Mini in portrait.
const WIDTHS = [320, 390, 412, 640, 768, 1024, 1440];
const PHONE = 600;

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await expect(page.locator(".oc-ribbon")).toBeAttached();
}

/// Everything visible in the shell's own rows that runs past the window's
/// right edge, unless it is inside something that scrolls sideways (a tab
/// list, a phone icon row): there it is reachable, not cut off.
const clipped = (page) =>
  page.evaluate(() => {
    const out = [];
    const rows = document.querySelectorAll(".app-header, .oc-ribbon, .rb-mobile");
    for (const row of rows) {
      if (getComputedStyle(row).display === "none") continue;
      for (const el of row.querySelectorAll("button, input, select")) {
        const r = el.getBoundingClientRect();
        if (!r.width || !r.height || el.closest("[hidden]")) continue;
        if (r.right <= innerWidth + 1 && r.left >= -1) continue;
        let n = el.parentElement, scrolls = false;
        while (n && n !== row.parentElement) {
          const ox = getComputedStyle(n).overflowX;
          if ((ox === "auto" || ox === "scroll") && n.scrollWidth > n.clientWidth + 1) { scrolls = true; break; }
          n = n.parentElement;
        }
        if (!scrolls) out.push(el.getAttribute("aria-label") || el.title || el.textContent.trim().slice(0, 24) || el.className);
      }
    }
    return out;
  });

for (const width of WIDTHS) {
  test(`the shell fits and every tab's commands stay reachable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 720 });
    await boot(page);

    expect(await page.evaluate(() => document.documentElement.scrollWidth),
      "the shell widens the page").toBeLessThanOrEqual(width + 1);
    expect(await clipped(page), "a control is cut off by the window edge").toEqual([]);

    const tabs = await page.locator(".rb-tab").evaluateAll((ts) => ts.map((t) => t.id.replace("rb-tab-", "")));
    expect(tabs.length, "no tabs, so this would prove nothing").toBeGreaterThanOrEqual(8);

    for (const id of tabs) {
      if (width <= PHONE) {
        // Excel mobile: the picker opens the tab chips, a chip picks the tab.
        await page.locator(".rb-mpick").click();
        const chip = page.locator(".rb-mchip").filter({ hasText: new RegExp(`^${await page.locator(`#rb-tab-${id}`).textContent()}$`) });
        await chip.scrollIntoViewIfNeeded();
        await chip.click();
        await expect(chip).toHaveAttribute("aria-pressed", "true");
        const sheet = await page.locator(".rb-msheet").boundingBox();
        expect(sheet.x, `${id}: the command sheet starts off-screen`).toBeGreaterThanOrEqual(0);
        expect(sheet.x + sheet.width, `${id}: the command sheet runs off-screen`).toBeLessThanOrEqual(width + 1);
        expect(await page.locator(".rb-mlist .rb-mitem").count(), `${id}: the tab offers nothing`).toBeGreaterThan(0);
        await page.locator(".rb-mpick").click();
      } else {
        const tab = page.locator(`#rb-tab-${id}`);
        await tab.scrollIntoViewIfNeeded();
        await tab.click();
        await expect(tab).toHaveAttribute("aria-selected", "true");
        // The band folds groups until it fits; whatever it cannot fold must
        // not be sliced off by the band's own `overflow: hidden`.
        const band = await page.evaluate(() => {
          const p = document.querySelector(".rb-panel:not([hidden])");
          return { over: p.scrollWidth - p.clientWidth, folded: p.querySelectorAll(".rb-group.is-folded").length };
        });
        expect(band.over, `${id}: the band is wider than its box and clips what is left`).toBeLessThanOrEqual(1);
      }
      expect(await clipped(page), `${id}: a control is cut off by the window edge`).toEqual([]);
    }
  });
}

test("a group the ribbon folded away still opens its commands, on screen", async ({ page }) => {
  await page.setViewportSize({ width: 768, height: 720 });
  await boot(page);
  await page.locator("#rb-tab-home").click();
  const folded = page.locator(".rb-panel:not([hidden]) .rb-group.is-folded .rb-folded").last();
  await expect(folded, "768px cannot fit all of Home, so something must fold").toBeVisible();
  await folded.click();
  const pop = page.locator(".rb-fold-pop");
  await expect(pop).toBeVisible();
  const box = await pop.boundingBox();
  expect(box.x, "the folded group's panel starts off-screen").toBeGreaterThanOrEqual(0);
  expect(box.x + box.width, "nor runs past the right edge").toBeLessThanOrEqual(769);
  expect(box.height, "nor is squashed to a sliver").toBeGreaterThan(24);
  // Its contents must fit it, not just the panel the window.
  const spill = await page.evaluate(() => {
    const f = document.querySelector(".rb-fold-pop");
    const r = f.getBoundingClientRect();
    return [...f.querySelectorAll("button, select, input")]
      .filter((e) => e.offsetParent !== null && e.getBoundingClientRect().right > r.right + 1)
      .map((e) => e.getAttribute("aria-label") || e.textContent.trim().slice(0, 20));
  });
  expect(spill, "no control may spill out of the folded group's panel").toEqual([]);
  expect(await pop.locator("button").count(), "the folded panel is empty").toBeGreaterThan(0);
});

test("on the narrowest phone, the last tab is still reachable and runs", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await boot(page);
  await page.locator(".rb-mpick").click();
  const last = page.locator(".rb-mchip").last();
  await last.scrollIntoViewIfNeeded();
  const box = await last.boundingBox();
  expect(box.x + box.width, "the last tab chip cannot be scrolled on screen").toBeLessThanOrEqual(321);
  await last.click();
  await expect(last).toHaveAttribute("aria-pressed", "true");
  // Undo never moves: the title bar keeps it at every width.
  await expect(page.locator('.rb-qat [data-cmd="edit.undo"]')).toBeVisible();
});
