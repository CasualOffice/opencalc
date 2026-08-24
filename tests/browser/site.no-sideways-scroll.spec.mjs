// The site must never scroll sideways, and nothing may be cut off instead.
//
// The landing page was 763px wide in a 390px viewport: every phone visitor got
// a page that slid horizontally. The `<pre>` blocks set their own width and the
// cards grew to fit — `overflow-x: auto` was already on some of them and never
// engaged, because a grid item defaults to `min-width: auto`, meaning "do not
// shrink below your content" (`UX-SITE-03`).
//
// Two assertions, and the second is the one that matters. Stopping the page
// from widening is easy — clip it, and the overflow becomes invisible instead
// of scrollable. So this also checks that anything still wider than the
// viewport sits inside something that can actually scroll it into view.

import { expect, test } from "@playwright/test";

const SIZES = [
  { name: "phone", width: 390, height: 844 },
  { name: "tablet", width: 768, height: 1024 },
  { name: "desktop", width: 1440, height: 900 },
];

for (const size of SIZES) {
  for (const page_ of ["index", "deploy", "docs"]) {
    test(`${page_}.html does not scroll sideways on ${size.name}`, async ({ page }) => {
      await page.setViewportSize({ width: size.width, height: size.height });
      await page.goto(`/${page_}.html`, { waitUntil: "networkidle" });
      await page.waitForTimeout(500);

      const result = await page.evaluate(() => {
        const unreachable = [];
        for (const el of document.querySelectorAll("*")) {
          const box = el.getBoundingClientRect();
          if (box.width === 0 || box.right <= window.innerWidth + 1) continue;
          // Something wider than the viewport is fine *if* an ancestor scrolls.
          let n = el.parentElement;
          let scrollable = false;
          while (n && n !== document.documentElement) {
            const ox = getComputedStyle(n).overflowX;
            if ((ox === "auto" || ox === "scroll") && n.scrollWidth > n.clientWidth + 1) {
              scrollable = true;
              break;
            }
            n = n.parentElement;
          }
          if (!scrollable) {
            unreachable.push(`${el.tagName.toLowerCase()}.${String(el.className || "").split(" ")[0]}`);
          }
        }
        return {
          docWidth: document.documentElement.scrollWidth,
          viewport: window.innerWidth,
          unreachable: [...new Set(unreachable)],
        };
      });

      expect(result.docWidth, "the document is no wider than the window").toBeLessThanOrEqual(
        result.viewport + 1,
      );
      expect(
        result.unreachable,
        "wide content must live in something that scrolls, not be cut off",
      ).toEqual([]);
    });
  }
}
