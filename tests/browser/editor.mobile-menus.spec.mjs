// A submenu has to open where the finger can reach it.
//
// `positionSub` computed its flip from `sub.offsetWidth` / `sub.offsetHeight`
// while `sub` was still `hidden` — and `[hidden]` is `display: none`, so both
// were **0**. The right-edge test `left + sw > innerWidth - 4` therefore
// compared the trigger's right edge against the window with no panel width in
// it, and could never be true; the bottom clamp likewise. On a desktop there is
// always room to the right, so nothing looked wrong for as long as this existed.
//
// On a 390px phone it made every one of the fourteen submenus unreachable.
// Measured before the fix, tapping Format ▸ Number:
//
//     {"opened":true,"x":377.3,"right":555.3,"w":178,"innerWidth":390,
//      "pxOffRight":165.3,"pxOffBottom":50}
//
// 178px of panel with 12.7px on screen: the user taps "Number ▸" and sees
// nothing happen. Number formats, Alignment, Text overflow, Freeze, Zoom,
// Clear, Fill, Chart, Sort range, Group, Protection, Trace and Download all
// live behind one.
//
// Retaken for the ribbon shell (UX-SHELL-21). The menu bar these were written
// against is parked off-screen there; on a phone the commands live in Excel
// mobile's bottom bar, whose sheet lists a tab's commands and whose rows open
// the same kind of submenu (`.rb-pop`, or a toolbar control's own popup). The
// three claims carry over unchanged: every submenu lands on screen, a tap on a
// row that opens one runs nothing, and one taller than the room scrolls.
//
// Driven by real touch through CDP rather than `.click()`: the bug is about
// where a panel lands under a finger on a phone, and the desktop click path is
// the one that never showed it.

import { expect, test } from "@playwright/test";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(300);
}

/** A real finger: the browser delivers it, rather than JS dispatching a click. */
async function tap(page, cdp, x, y) {
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  await page.waitForTimeout(40);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await page.waitForTimeout(250);
}

/// Open the command sheet on a tab, by touch.
async function openTab(page, cdp, label) {
  const pick = await page.locator(".rb-mpick").boundingBox();
  if (await page.locator(".rb-msheet").isHidden()) await tap(page, cdp, pick.x + pick.width / 2, pick.y + pick.height / 2);
  const chip = page.locator(".rb-mchip", { hasText: new RegExp(`^${label}$`) });
  await chip.scrollIntoViewIfNeeded();
  const cb = await chip.boundingBox();
  await tap(page, cdp, cb.x + cb.width / 2, cb.y + cb.height / 2);
  await expect(chip).toHaveAttribute("aria-pressed", "true");
}

/// The submenu a row opened: the ribbon's own popover, or a toolbar control's
/// popup re-anchored under it. Whichever is visible.
const openSub = (page) =>
  page.evaluate(() => {
    const s = [...document.querySelectorAll(".rb-pop, .toolbar .popmenu")]
      .find((e) => !e.hidden && getComputedStyle(e).display !== "none" && e.getBoundingClientRect().height > 0);
    if (!s) return null;
    const r = s.getBoundingClientRect();
    return {
      top: +r.top.toFixed(1), bottom: +r.bottom.toFixed(1), left: +r.left.toFixed(1), right: +r.right.toFixed(1),
      offRight: +Math.max(0, r.right - innerWidth).toFixed(1), offLeft: +Math.max(0, -r.left).toFixed(1),
      offBottom: +Math.max(0, r.bottom - innerHeight).toFixed(1), offTop: +Math.max(0, -r.top).toFixed(1),
      innerHeight, scrollHeight: s.scrollHeight, clientHeight: s.clientHeight, overflowY: getComputedStyle(s).overflowY,
      items: s.querySelectorAll("button").length,
    };
  });

/// Tap a sheet row by command id, scrolling the list to it first.
async function tapRow(page, cdp, id) {
  const row = page.locator(`.rb-mlist .rb-mitem[data-cmd="${id}"]`);
  await row.scrollIntoViewIfNeeded();
  const b = await row.boundingBox();
  await tap(page, cdp, b.x + b.width / 2, b.y + b.height / 2);
}

test("every phone submenu opens inside the screen", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);
  const tabs = await page.locator(".rb-mchip[data-tab]").allTextContents();
  expect(tabs.length, "no tabs, so this would prove nothing").toBeGreaterThan(3);

  const offscreen = [];
  let checked = 0;
  for (const label of tabs) {
    await openTab(page, cdp, label);
    const ids = await page.locator('.rb-mlist .rb-mitem[aria-haspopup="true"]').evaluateAll((rs) => rs.map((r) => r.dataset.cmd));
    for (const id of ids) {
      await openTab(page, cdp, label);
      await tapRow(page, cdp, id);
      const sub = await openSub(page);
      checked += 1;
      expect(sub, `${label} ▸ ${id} opened nothing`).not.toBeNull();
      if (sub.offRight || sub.offLeft || sub.offBottom || sub.offTop) offscreen.push(`${label} ▸ ${id}: ${JSON.stringify(sub)}`);
      await page.keyboard.press("Escape");
      await page.waitForTimeout(60);
    }
  }
  expect(checked, "no submenu was opened, so this would prove nothing").toBeGreaterThan(8);
  expect(offscreen, `${checked} submenus opened; these landed off the screen`).toEqual([]);
});

test("tapping a submenu row opens it and runs nothing", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "keep me");
    window.opencalcEditor.selectForTest(0, 0);
  });
  const cdp = await page.context().newCDPSession(page);

  // A tap is not just a click: Chrome replays the mouse sequence at the touch
  // point, and on the old menu bar the `mouseenter` opened the submenu under
  // the finger so the `click` that followed ran one of its items — tapping
  // "Clear ▸" cleared the cell. The cell is the assertion, because that is the
  // part a user loses.
  await openTab(page, cdp, "Home");
  await tapRow(page, cdp, "edit.clear");

  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0)),
    "one tap on a submenu row wiped the cell",
  ).toBe("keep me");
  const sub = await openSub(page);
  expect(sub, "and the submenu it was supposed to open is not open").not.toBeNull();
  expect(sub.items, "Clear offers its three choices").toBe(3);
});

test("a submenu taller than the room below it scrolls instead of running off the bottom", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);

  // Home ▸ Format is the longest menu on the phone, and its row sits in a
  // sheet docked to the bottom of the screen — the least room below of any.
  await openTab(page, cdp, "Home");
  await tapRow(page, cdp, "cells.format");
  const s = await openSub(page);
  expect(s, "Format opened nothing").not.toBeNull();
  expect(s.items, "an empty submenu proves nothing").toBeGreaterThan(3);
  expect(s.top, `submenu top ${s.top}`).toBeGreaterThanOrEqual(-0.5);
  expect(s.bottom, `submenu bottom ${s.bottom} of ${s.innerHeight}`).toBeLessThanOrEqual(s.innerHeight + 0.5);
  // Clipping and fitting look identical from the rect alone: if the panel was
  // clamped, what did not fit has to still be reachable.
  if (s.scrollHeight > s.clientHeight + 1) expect(s.overflowY).toBe("auto");
});
