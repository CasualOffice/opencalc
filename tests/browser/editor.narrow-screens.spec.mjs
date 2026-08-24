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

  expect(scrolled.hidden, "there is toolbar beyond the edge to reach").toBeGreaterThan(0);
  expect(scrolled.landed, "and it can actually be scrolled to").toBeGreaterThan(0);
});
