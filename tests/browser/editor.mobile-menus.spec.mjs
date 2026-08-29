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
// Driven by real touch through CDP rather than `.click()`: the bug is about
// where a panel lands under a finger on a phone, and the desktop click path is
// the one that never showed it.

import { expect, test } from "@playwright/test";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
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

test("every menu-bar submenu opens inside the screen", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);

  const tops = await page.locator(".menubar .menu-top:not([hidden])").all();
  expect(tops.length, "no menus, so this would prove nothing").toBeGreaterThan(3);

  const offscreen = [];
  let checked = 0;
  for (const top of tops) {
    const label = (await top.textContent()).trim();
    const tb = await top.boundingBox();
    if (!tb) continue;
    await tap(page, cdp, tb.x + tb.width / 2, tb.y + tb.height / 2);

    // Every item in this drop that owns a submenu, by index — the drop is
    // rebuilt on each open, so a held handle would go stale.
    const count = await page.evaluate(() => {
      const d = [...document.querySelectorAll(".menu-drop")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
      return d ? [...d.querySelectorAll(":scope > button")].filter((b) => b.querySelector(".mi-caret")).length : 0;
    });
    await page.keyboard.press("Escape");

    for (let i = 0; i < count; i += 1) {
      await tap(page, cdp, tb.x + tb.width / 2, tb.y + tb.height / 2);
      const at = await page.evaluate((i) => {
        const d = [...document.querySelectorAll(".menu-drop")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
        const b = [...d.querySelectorAll(":scope > button")].filter((x) => x.querySelector(".mi-caret"))[i];
        const r = b.getBoundingClientRect();
        return { name: b.textContent.trim(), x: r.x + r.width / 2, y: r.y + r.height / 2 };
      }, i);
      await tap(page, cdp, at.x, at.y);

      const sub = await page.evaluate(() => {
        const s = [...document.querySelectorAll(".menu-sub")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
        if (!s) return null;
        const r = s.getBoundingClientRect();
        return {
          left: +r.left.toFixed(1), right: +r.right.toFixed(1), top: +r.top.toFixed(1), bottom: +r.bottom.toFixed(1),
          offRight: +Math.max(0, r.right - window.innerWidth).toFixed(1),
          offLeft: +Math.max(0, -r.left).toFixed(1),
          offBottom: +Math.max(0, r.bottom - window.innerHeight).toFixed(1),
          offTop: +Math.max(0, -r.top).toFixed(1),
        };
      });
      checked += 1;
      expect(sub, `${label} ▸ ${at.name} opened nothing`).not.toBeNull();
      if (sub.offRight || sub.offLeft || sub.offBottom || sub.offTop) {
        offscreen.push(`${label} ▸ ${at.name}: ${JSON.stringify(sub)}`);
      }
      await page.keyboard.press("Escape");
      await page.waitForTimeout(80);
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

  // A tap is not just a click. Chrome replays the whole mouse sequence at the
  // touch point, `mouseenter` first — and `mouseenter` opened the submenu. On a
  // 390px screen the submenu has nowhere to go but back over the row that
  // opened it, so the `click` that arrived a moment later landed on whichever
  // *submenu item* was now under the finger and ran it. Tapping "Clear ▸"
  // cleared the cell.
  //
  // The cell content is the assertion rather than the menu state, because that
  // is the part a user loses.
  const edit = page.locator(".menubar .menu-top", { hasText: /^Edit$/ }).first();
  const eb = await edit.boundingBox();
  await tap(page, cdp, eb.x + eb.width / 2, eb.y + eb.height / 2);
  const at = await page.evaluate(() => {
    const d = [...document.querySelectorAll(".menu-drop")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
    const b = [...d.querySelectorAll(":scope > button")].find((x) => x.textContent.startsWith("Clear"));
    const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  });
  await tap(page, cdp, at.x, at.y);

  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0)),
    "one tap on a submenu row wiped the cell",
  ).toBe("keep me");
  const sub = await page.evaluate(() => {
    const s = [...document.querySelectorAll(".menu-sub")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
    return s ? s.dataset.ocFor : null;
  });
  expect(sub, "and the submenu it was supposed to open is still open").toBe("edit.clear");
});

test("a submenu taller than the room below it scrolls instead of running off the bottom", async ({ page }) => {
  await boot(page);
  const cdp = await page.context().newCDPSession(page);

  // Format ▸ Number is the longest submenu in the bar and hangs from an item
  // low in the longest drop, so it is the one with the least room under it.
  const fmt = page.locator(".menubar .menu-top", { hasText: /^Format$/ }).first();
  const fb = await fmt.boundingBox();
  await tap(page, cdp, fb.x + fb.width / 2, fb.y + fb.height / 2);
  const at = await page.evaluate(() => {
    const d = [...document.querySelectorAll(".menu-drop")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
    const b = [...d.querySelectorAll(":scope > button")].find((x) => x.textContent.includes("Number"));
    const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  });
  await tap(page, cdp, at.x, at.y);

  const s = await page.evaluate(() => {
    const e = [...document.querySelectorAll(".menu-sub")].find((x) => !x.hidden && x.getBoundingClientRect().height > 0);
    const r = e.getBoundingClientRect();
    return {
      bottom: +r.bottom.toFixed(1), top: +r.top.toFixed(1), innerHeight: window.innerHeight,
      scrollHeight: e.scrollHeight, clientHeight: e.clientHeight, overflowY: getComputedStyle(e).overflowY,
      items: e.querySelectorAll("button").length,
    };
  });
  expect(s.items, "an empty submenu proves nothing").toBeGreaterThan(3);
  expect(s.top, `submenu top ${s.top}`).toBeGreaterThanOrEqual(-0.5);
  expect(s.bottom, `submenu bottom ${s.bottom} of ${s.innerHeight}`).toBeLessThanOrEqual(s.innerHeight + 0.5);
  // Clipping and fitting look identical from the rect alone: if the panel was
  // clamped, what did not fit has to still be reachable.
  if (s.scrollHeight > s.clientHeight + 1) expect(s.overflowY).toBe("auto");
});
