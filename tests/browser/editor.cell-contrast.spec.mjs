// A cell's text has to contrast with the cell, not with the theme.
//
// Reported from a running editor: "font color change on dark light mode change
// but not bg of cell .. making it harder to see". A cell's fill comes from the
// *file*, so it is the same colour in either theme, while the text colour was
// read from the theme — so switching to dark mode put near-white text on an
// authored pale fill and the contents disappeared.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const inkOn = (page, bg) =>
  page.evaluate((fill) => window.opencalcEditor.cellFg({ r: 0, c: 0, bg: fill }), bg);

test("text on a filled cell is chosen against the fill, in either theme", async ({ page }) => {
  await boot(page);

  // A pale fill needs dark ink and a dark fill needs light ink — whatever the
  // theme says, because the fill is what the text actually sits on.
  const onPale = await inkOn(page, "FFFF00");
  const onDark = await inkOn(page, "1A1A2E");
  expect(onPale.toLowerCase(), "dark ink on a yellow fill").toBe("#111418");
  expect(onDark.toLowerCase(), "light ink on a navy fill").toBe("#ffffff");

  // The same answers after a theme switch: the fill did not change, so the ink
  // must not either. This is the case that regressed.
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  await page.evaluate(() => window.opencalcEditor.refreshTheme());
  expect(await inkOn(page, "FFFF00"), "still dark ink in dark mode").toBe(onPale);
  expect(await inkOn(page, "1A1A2E"), "still light ink in dark mode").toBe(onDark);
});

test("an explicit font colour still wins over the fill", async ({ page }) => {
  await boot(page);
  // The author paired that colour with that fill deliberately; repainting it
  // would be second-guessing somebody's formatting.
  const ink = await page.evaluate(() =>
    window.opencalcEditor.cellFg({ r: 0, c: 0, bg: "FFFF00", fc: "FF0000" }));
  expect(ink.toLowerCase()).toBe("#ff0000");
});
