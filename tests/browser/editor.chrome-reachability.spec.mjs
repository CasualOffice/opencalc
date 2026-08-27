// No control the chrome offers may be unreachable — at any width.
//
// The toolbar has collapsed progressively since it was written, and it does it
// well: each group folds into a "Label ▾" flyout, lowest priority first, never
// a scrollbar and never a second row. What it had no answer for was running
// out of groups. Fully collapsed the bar still measured 791px, so every
// viewport narrower than that clipped it with nothing left to fold — 401px of
// toolbar off-screen on a 390px phone, and 23px even on an iPad Mini. The menu
// bar had no collapse at all and simply sliced "Help" in half.
//
// Neither failure announced itself. A clipped control is not a broken one; it
// renders perfectly, right up to the edge of a viewport nobody tested at, and
// `editor.css` contained no width-based media query of any kind to suggest a
// width had ever been considered.
//
// So this asserts the invariant rather than any particular layout: at every
// width down to 320px, the toolbar fits its own box, and every top-level menu
// is reachable — visible on the bar, or inside the overflow that holds it.

import { expect, test } from "@playwright/test";

// 320 is the narrowest phone still sold; 768 is an iPad Mini in portrait, which
// the 791px floor also broke and which nobody would have called "mobile".
const WIDTHS = [320, 390, 412, 768, 1024, 1440];

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

for (const width of WIDTHS) {
  test(`chrome fits and stays reachable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 720 });
    await boot(page);

    const m = await page.evaluate(() => {
      const bar = document.querySelector(".toolbar");
      const clipped = (el) => el.getBoundingClientRect().right > window.innerWidth + 1;
      const menuButtons = [...document.querySelectorAll("button.menu-top")];
      return {
        overflow: bar.scrollWidth - bar.clientWidth,
        clippedMenus: menuButtons.filter((b) => !b.hidden && clipped(b)).map((b) => b.textContent.trim()),
        // Every menu, whether it sits on the bar or has been pushed into the
        // overflow — the invariant is reachability, not visibility.
        reachable: menuButtons.length,
        onBar: menuButtons.filter((b) => !b.hidden).length,
        clippedTools: [...document.querySelectorAll(".toolbar .tb-btn")]
          .filter((b) => !b.hidden && b.offsetParent !== null && clipped(b)).length,
      };
    });

    expect(m.overflow, "the toolbar must fit its own box").toBeLessThanOrEqual(1);
    expect(m.clippedMenus, "no menu may be sliced by the viewport edge").toEqual([]);
    expect(m.clippedTools, "no toolbar button may sit past the viewport edge").toBe(0);
    // The full set still exists at every width; narrow ones just relocate it.
    expect(m.reachable, "every menu still exists").toBeGreaterThanOrEqual(8);
  });
}

test("a menu pushed into the overflow still opens", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await boot(page);
  const more = page.locator("#menu-more");
  await expect(more, "320px cannot fit eight menus, so the overflow must appear").toBeVisible();
  await more.click();
  // Help is the first to be pushed out, and it must still open from in here.
  await page.locator("#menu-more-drop button", { hasText: "Help" }).click();
  await expect(page.locator(".menu-drop:not([hidden])")).toBeVisible();
});

test("toolbar tools pushed into the overflow are still there", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await boot(page);
  const more = page.locator("#tb-more");
  await expect(more, "the toolbar runs out of groups to fold at 320px").toBeVisible();
  await more.click();
  await expect(page.locator("#tb-more-flyout")).toBeVisible();
  // Visible is not the same as on-screen, and this is the assertion that was
  // missing the first time: the flyout opened, passed a visibility check, and
  // was being clipped to the toolbar band by an `overflow: hidden` I had added
  // to the bar it lives inside. It rendered as two stray letters at the edge.
  const box = await page.locator("#tb-more-flyout").boundingBox();
  const vw = page.viewportSize().width;
  expect(box.x, "the overflow panel must not start off-screen").toBeGreaterThanOrEqual(0);
  expect(box.x + box.width, "nor run past the right edge").toBeLessThanOrEqual(vw + 1);
  expect(box.height, "nor be squashed to a sliver").toBeGreaterThan(24);
  // And its *contents* must fit it. The panel itself measured inside the
  // viewport while the buttons within it ran off the right-hand edge, because
  // it had been laid out as a row of full-width labelled buttons.
  const spill = await page.evaluate(() => {
    const f = document.getElementById("tb-more-flyout");
    const r = f.getBoundingClientRect();
    return [...f.querySelectorAll("button, select, input")]
      .filter((e) => e.offsetParent !== null && e.getBoundingClientRect().right > r.right + 1)
      .map((e) => e.id || e.textContent.trim().slice(0, 20));
  });
  expect(spill, "no control may spill out of the overflow panel").toEqual([]);
  // Undo never moves: it is the one control that must survive every width.
  await expect(page.locator("#tb-undo")).toBeVisible();
});

test("the menu overflow sits at the end of the bar, not the start", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 720 });
  await boot(page);
  // Placed first in the markup and the menus appended after it, the button
  // rendered to the *left* of every menu it was overflowing.
  const order = await page.evaluate(() => {
    const bar = document.getElementById("menubar");
    const kids = [...bar.children].filter((e) => !e.hidden);
    return kids.map((e) => e.id || e.dataset.ocLabel || e.textContent.trim());
  });
  // Not the last child of the bar — `hdr-collapse` legitimately owns the far
  // right — but after every menu it stands in for, which is the thing that was
  // wrong: placed first in the markup, it rendered to the *left* of them all.
  const lastMenu = order.map((x, i) => [x, i]).filter(([x]) => x !== "menu-more" && x !== "hdr-collapse").pop();
  expect(order.indexOf("menu-more"), `bar order was ${JSON.stringify(order)}`).toBeGreaterThan(lastMenu[1]);
});
