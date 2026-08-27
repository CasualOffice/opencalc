// The editor must fit the window it is given.
//
// It had no width breakpoint of any kind — only preference queries for dark
// mode, reduced motion and contrast. `.menubar` and `.toolbar` are unwrapped
// flex rows, so on a phone they ran past the edge and widened the whole
// document to 779px: the page slid sideways, and half the toolbar could only be
// reached by dragging the entire editor across (`UX-EDIT-01`).
//
// The fix is that they scroll within themselves, so this checks the two things
// that distinguishes from clipping: the document stays the width of the window,
// and everything wider than the window is still reachable.

import { expect, test } from "@playwright/test";

const SIZES = [
  { name: "phone", width: 390, height: 844 },
  { name: "tablet", width: 768, height: 1024 },
];

for (const size of SIZES) {
  test(`the editor fits a ${size.name} window`, async ({ page }) => {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/editor.html");
    await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
    await page.waitForTimeout(800);

    const result = await page.evaluate(() => {
      const unreachable = [];
      for (const el of document.querySelectorAll("*")) {
        const box = el.getBoundingClientRect();
        if (box.width === 0 || box.right <= window.innerWidth + 1) continue;
        let n = el.parentElement;
        let ok = false;
        while (n && n !== document.documentElement) {
          const ox = getComputedStyle(n).overflowX;
          if ((ox === "auto" || ox === "scroll") && n.scrollWidth > n.clientWidth + 1) {
            ok = true;
            break;
          }
          n = n.parentElement;
        }
        if (!ok) unreachable.push(`${el.tagName.toLowerCase()}.${String(el.className || "").split(" ")[0]}`);
      }
      return {
        docWidth: document.documentElement.scrollWidth,
        viewport: window.innerWidth,
        unreachable: [...new Set(unreachable)],
      };
    });

    expect(result.docWidth, "the editor does not widen the page").toBeLessThanOrEqual(
      result.viewport + 1,
    );
    expect(
      result.unreachable,
      "toolbar controls past the edge must be reachable, not cut off",
    ).toEqual([]);
  });
}

test("the toolbar can be scrolled to its far end on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.waitForTimeout(800);

  // Asserting the *mechanism*, not just the absence of overflow: a toolbar that
  // had been clipped would also report nothing unreachable above.
  const scrolled = await page.evaluate(() => {
    const tb = document.querySelector(".toolbar");
    const hidden = tb.scrollWidth - tb.clientWidth;
    tb.scrollLeft = 9999;
    return { hidden, landed: tb.scrollLeft };
  });

  // Superseded by UX-MOB-01, and the assertion is inverted rather than deleted
  // so the change of mechanism is on the record.
  //
  // UX-EDIT-01 chose "collapse the groups, then scroll" deliberately, and ruled
  // out wrapping for a good reason: wrapping costs height, and every pixel of it
  // comes out of the grid. A `⋯` overflow costs no height either — the panel
  // floats — and it does the one thing a scrollbar here could not, which is say
  // that there is more. That row also notes the scrollbar is deliberately
  // hidden, because chrome with a visible track loses 8-15px of a 48px bar. A
  // hidden scrollbar on a toolbar is not an affordance: nothing on screen
  // suggests dragging it, and on a phone the gesture competes with the grid's
  // own horizontal pan.
  //
  // So the contract is now the stronger one: there is nothing past the edge to
  // reach. Scrolling stays wired up underneath, and if anything ever does
  // overflow again it must still be reachable — that half of UX-EDIT-01 holds.
  expect(scrolled.hidden, "the fold leaves nothing past the edge").toBe(0);
  if (scrolled.hidden > 0) {
    expect(scrolled.landed, "and anything that did would still be scrollable").toBeGreaterThan(0);
  }
  // What used to need scrolling is in here instead, named rather than hidden.
  await expect(page.locator("#tb-more")).toBeVisible();
});
