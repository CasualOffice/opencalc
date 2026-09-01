// A menu row is sized for the pointer that is actually pointing.
//
// `UX-CHR-13`. The cell context menu has 41 rows. At `8px 10px` each one was
// 33px tall and the panel came to **638px** — taller than the window on a
// laptop, and more than half of it padding. Excel's own context menu row is
// about 24px and macOS's is about 22, so 33 was not a house style; it was a
// control sized for a finger and then shown to a mouse.
//
// `editor.touch-targets.spec.mjs` is the other half of this and must keep
// passing: with `pointer: coarse` these same rows are pinned to a 44px
// minimum, which is the constraint with a reason behind it. Neither number is
// free to drift into the other's case, so both are asserted — this file with a
// mouse, that one with a finger.
//
// The ceiling is 28px rather than 25: the assertion is that rows are compact,
// not that a font stack renders to the pixel this machine happened to produce.

import { expect, test } from "@playwright/test";

/// Comfortably above the 25px this renders at, and far below the 33px that
/// prompted the row. A change that puts the finger's metric back on the mouse
/// lands at 33 and fails here; a font that rounds a pixel differently does not.
const CEILING = 28;

test("context menu rows are compact with a mouse", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  await page.locator("canvas").first()
    .click({ button: "right", position: { x: 120, y: 90 } });

  const rows = await page.evaluate(() => {
    const menu = [...document.querySelectorAll(".ctx-menu")].find((e) => !e.hidden);
    if (!menu) return null;
    return {
      count: menu.querySelectorAll("button").length,
      tallest: Math.max(...[...menu.querySelectorAll("button")]
        .map((b) => b.getBoundingClientRect().height)),
      panel: menu.getBoundingClientRect().height,
    };
  });

  expect(rows, "the right-click menu did not open").not.toBeNull();
  expect(rows.count, "a context menu with no rows proves nothing").toBeGreaterThan(5);
  expect(
    rows.tallest,
    `the tallest context menu row is ${rows.tallest.toFixed(1)}px, and the panel is `
    + `${rows.panel.toFixed(1)}px for ${rows.count} rows. A row over ${CEILING}px with a mouse `
    + `is the finger's 44px metric leaking into the pointer case.`,
  ).toBeLessThanOrEqual(CEILING);
});
