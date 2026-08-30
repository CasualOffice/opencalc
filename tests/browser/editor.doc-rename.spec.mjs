// Renaming the document from the strip, and the logo that belongs there
// (`UX-CHR-10`).
//
// `UX-CHR-03` moved the strip from naming the *product* to naming the
// *document*, which was right and is what every competitor does. It then took
// two things too far, both reported from a running editor:
//
//   - the document's name was displayed and could not be **changed**. Excel,
//     Sheets, OnlyOffice and LibreOffice all rename from the title area; a name
//     you can read and not edit is a caption, and the only route left here was
//     Save As, which is a different operation with different consequences.
//   - the logo went with the wordmark. A browser tab has no chrome of its own,
//     so the mark is the only thing identifying which application is running,
//     and every web application keeps one. What `docs/88` measured was that no
//     competitor spends a strip *naming the product in words* — the wordmark,
//     the version and the `Alpha` pill. A mark is not that.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
}

test("the document can be renamed from the strip", async ({ page }) => {
  await boot(page);
  await expect(page.locator("#doc-name")).toHaveText("Untitled workbook");

  await page.click("#doc-name");
  const input = page.locator("#doc-rename");
  await expect(input, "clicking the name did not open an editable field").toBeVisible();

  await input.fill("Quarterly figures");
  await input.press("Enter");
  await page.waitForTimeout(300);

  await expect(page.locator("#doc-name"), "the new name is not shown in the strip").toHaveText("Quarterly figures");
  expect(await page.evaluate(() => window.opencalcEditor.documentName()),
    "the strip shows a name the document does not have").toBe("Quarterly figures");
});

/// **The field opens holding the name the strip was showing.**
///
/// `documentName()` is `null` until a file has been opened, so seeding from it
/// replaced "Untitled workbook" with an empty box on every new workbook — the
/// label vanished and nothing legible took its place, which reads as a control
/// that does not work. Reported exactly that way.
test("the rename field opens with the name already in it", async ({ page }) => {
  await boot(page);
  const shown = (await page.locator("#doc-name").textContent()).trim();
  await page.click("#doc-name");
  const input = page.locator("#doc-rename");
  await expect(input).toBeVisible();
  expect(await input.inputValue(),
    `the field opened empty while the strip was showing "${shown}"`).toBe(shown);

  // And it occupies roughly the label's own footprint, so the strip does not
  // jump sideways under the pointer that just clicked it.
  const box = await input.boundingBox();
  expect(box.width, "the rename field is far wider than the name it replaced").toBeLessThan(460);
});

/// **Escape restores, and does not commit half a rename.**
test("Escape abandons a rename", async ({ page }) => {
  await boot(page);
  await page.click("#doc-name");
  await page.locator("#doc-rename").fill("Half typed");
  await page.locator("#doc-rename").press("Escape");
  await page.waitForTimeout(200);
  await expect(page.locator("#doc-name")).toHaveText("Untitled workbook");
  expect(await page.evaluate(() => window.opencalcEditor.documentName())).toBe(null);
});

/// **A name is somebody else's text.**
///
/// It reaches the strip, the tab title and — on the desktop — a native window
/// title. `textContent` is what keeps that safe, and this is the test that says
/// so rather than the comment.
test("a name containing markup is shown, not executed", async ({ page }) => {
  await boot(page);
  await page.click("#doc-name");
  await page.locator("#doc-rename").fill("<img src=x onerror=alert(1)>Report");
  await page.locator("#doc-rename").press("Enter");
  await page.waitForTimeout(250);
  await expect(page.locator("#doc-name")).toHaveText("<img src=x onerror=alert(1)>Report");
  expect(await page.evaluate(() => document.querySelector("#doc-name").querySelector("img")),
    "the name was parsed as markup").toBe(null);
});

/// **An empty rename is not a rename.**
test("clearing the name leaves the document as it was", async ({ page }) => {
  await boot(page);
  await page.click("#doc-name");
  await page.locator("#doc-rename").fill("   ");
  await page.locator("#doc-rename").press("Enter");
  await page.waitForTimeout(250);
  await expect(page.locator("#doc-name")).toHaveText("Untitled workbook");
});

/// **The mark is back; the wordmark, version and pill are not.**
test("the strip carries a logo and still names no product in words", async ({ page }) => {
  await boot(page, "?brand=Ledgerly");
  await expect(page.locator(".brand-logo"), "there is no logo in the strip").toBeVisible();

  const said = await page.evaluate(() => ({
    strip: document.querySelector(".app-header").textContent.replace(/\s+/g, " ").trim(),
    wordmarks: document.querySelectorAll(".tb-brand, .badge").length,
  }));
  expect(said.wordmarks, "the wordmark or version pill came back with the logo").toBe(0);
  expect(said.strip, `the strip names a product in words: ${said.strip}`).not.toMatch(/Ledgerly|OpenCalc/);
});
