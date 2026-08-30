// White-labelling, in a browser.
//
// An integrator reselling a spreadsheet editor cannot ship one with somebody
// else's name in it — every product in this market has this, and it is what the
// WOPI adapter configures when it points its iframe at the editor (docs/74).
//
// **What carries the brand changed in `UX-CHR-03`, and this file changed with
// it.** It used to be `.tb-brand`, the wordmark in the branding strip, and
// these tests asserted the strip showed the configured name. That strip names
// the *document* now: none of Excel, LibreOffice Calc, OnlyOffice, Google
// Sheets or Numbers names its product inside the document window, and an
// integrator wants no product name in the chrome even more than they want their
// own in it. So the brand's surfaces are the two every one of those five uses —
// the window (here, the tab's title) and Help ▸ About — and the strongest thing
// this file can now assert is the *absence* of a wordmark in the chrome
// together with the presence of the brand where a user goes to look for it.
//
// This is a browser gate rather than a unit test because the whole feature is
// "what does the page say", and the two ways it breaks are both in the DOM: a
// name that never reaches the user, and a name that reaches them as markup.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/** The text of Help ▸ About, opened by the id the native menu dispatches with. */
async function about(page) {
  await page.evaluate(() => {
    const id = window.opencalcEditor.listCommands().find((c) => c.startsWith("help.about"));
    window.opencalcEditor.runCommand(id);
  });
  await expect(page.locator(".oc-modal:not([hidden])")).toBeVisible();
  return {
    title: (await page.locator("#oc-modal-title").textContent()).trim(),
    body: (await page.locator("#oc-modal-body").textContent()).trim(),
  };
}

/// **A brand on the URL replaces the product name a user can see.**
test("the title and Help ▸ About carry the configured brand", async ({ page }) => {
  await boot(page, "?brand=Ledgerly");

  await expect(page).toHaveTitle("Ledgerly");
  const dialog = await about(page);
  expect(dialog.title).toBe("About Ledgerly");
  expect(dialog.body).toContain("Ledgerly");
});

/// **And no wordmark comes back into the chrome to disagree with it.**
///
/// The failure this guards is not cosmetic: a strip that still said *OpenCalc*
/// while the title said *Ledgerly* would put two product names on one screen,
/// which is worse for a reseller than either name alone.
test("no region of the chrome names a product", async ({ page }) => {
  await boot(page, "?brand=Ledgerly");

  const chrome = await page.evaluate(() => {
    const text = (sel) => document.querySelector(sel)?.textContent.replace(/\s+/g, " ").trim() ?? "";
    return {
      strip: text(".app-header"),
      menubar: text("#menubar"),
      statusbar: text(".bottom-bar"),
      // **`.brand-logo` is deliberately not in this list** (`UX-CHR-10`).
      // `UX-CHR-03` forbade all three and that went one item too far: what
      // `docs/88` measured was that no competitor spends a strip *naming the
      // product in words*. A mark is not a name, a browser tab has no chrome
      // of its own to carry one, and the strip lost the only thing on screen
      // identifying which application was running. The wordmark and the
      // version pill stay out, and the text assertion below is what actually
      // holds the line — it fails on the product's name appearing anywhere in
      // the chrome, however it got there.
      wordmarks: document.querySelectorAll(".tb-brand, .badge").length,
    };
  });
  expect(chrome.wordmarks, "a wordmark or version pill is back in the chrome").toBe(0);
  for (const [region, said] of Object.entries(chrome)) {
    if (region === "wordmarks") continue;
    expect(said, `${region} names a product: ${said}`).not.toMatch(/Ledgerly|OpenCalc/);
  }
});

/// **Unconfigured, it is OpenCalc.**
///
/// The default has to survive, because the overwhelming majority of deployments
/// never set this and a nameless About dialog would be the regression.
test("an unbranded page keeps the product name where the brand lives", async ({ page }) => {
  await boot(page);
  const dialog = await about(page);
  expect(dialog.title).toBe("About OpenCalc");
});

/// **A brand is text, not markup.**
///
/// It arrives on a URL, so anybody who can hand a user a link chooses it. The
/// dialog is where this matters most and always did: `BRAND` reaches
/// `innerHTML` there, which is the one place in this feature where a name
/// becomes markup unless something stops it.
test("a brand containing markup is shown, not executed", async ({ page }) => {
  const attack = "<img src=x onerror=\"window.__owned=1\">Acme";
  await boot(page, `?brand=${encodeURIComponent(attack)}`);

  const dialog = await about(page);
  // Shown verbatim: the tag is characters on the screen.
  expect(dialog.body).toContain(attack);
  expect(dialog.title).toBe(`About ${attack}`);
  // And nothing ran.
  expect(await page.evaluate(() => window.__owned)).toBeUndefined();
  expect(await page.locator("#oc-modal-body img").count()).toBe(0);
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
