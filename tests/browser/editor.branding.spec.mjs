// White-labelling, in a browser.
//
// An integrator reselling a spreadsheet editor cannot ship one with somebody
// else's name in the toolbar — every product in this market has this, and it is
// what the WOPI adapter configures when it points its iframe at the editor
// (docs/74).
//
// This is a browser gate rather than a unit test because the whole feature is
// "what does the page say", and the two ways it breaks are both in the DOM: a
// name that never reaches the toolbar, and a name that reaches it as markup.

import { expect, test } from "@playwright/test";

/// **A brand on the URL replaces the product name a user can see.**
test("the toolbar and title carry the configured brand", async ({ page }) => {
  await page.goto("/editor.html?brand=Ledgerly");

  await expect(page.locator(".tb-brand")).toHaveText("Ledgerly");
  await expect(page).toHaveTitle("Ledgerly");
});

/// **Unconfigured, it is OpenCalc.**
///
/// The default has to survive, because the overwhelming majority of
/// deployments never set this and a blank toolbar would be the regression.
test("an unbranded page keeps the product name", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator(".tb-brand")).toHaveText("OpenCalc");
});

/// **A brand is text, not markup.**
///
/// It arrives on a URL, so anybody who can hand a user a link chooses it. A
/// name written into `innerHTML` is a cross-site scripting hole reachable by
/// sending someone a link to their own editor.
test("a brand containing markup is shown, not executed", async ({ page }) => {
  const attack = "<img src=x onerror=\"window.__owned=1\">Acme";
  await page.goto(`/editor.html?brand=${encodeURIComponent(attack)}`);

  // Shown verbatim: the tag is characters on the screen.
  await expect(page.locator(".tb-brand")).toHaveText(attack);
  // And nothing ran.
  expect(await page.evaluate(() => window.__owned)).toBeUndefined();
  expect(await page.locator(".tb-brand img").count()).toBe(0);
});

/// **An accent colour is applied, and only if it is a colour.**
///
/// It lands in a CSS custom property. A value allowed to be arbitrary text can
/// close the declaration and open another, so anything that is not a colour is
/// ignored rather than sanitised — there is no useful half-measure.
test("an accent colour is applied when it is one", async ({ page }) => {
  await page.goto("/editor.html?accent=%23ff0055");
  const applied = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--oc-accent-color").trim());
  expect(applied).toBe("#ff0055");

  await page.goto("/editor.html?accent=red%3Bposition%3Afixed");
  const ignored = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--oc-accent-color").trim());
  expect(ignored).not.toContain("fixed");
});
