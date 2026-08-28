// Each mode is a different product, and the chrome has to know which one it is.
//
// Measured before this existed: the same editor booted standalone, as a desktop
// app and embedded presented **identical** chrome — 195 commands, File ▸ Open,
// six Download entries, Print, in every one. There was one chrome with one flag.
//
// That is not cosmetic. An editor embedded in somebody's page offered File ▸
// Open, so a user could replace the host's document from inside the host's own
// page; and six Download entries, so they could take a copy the host never
// authorised. Under WOPI the host owns save and versioning, and the editor's own
// Save would disagree with it.
//
// Modes are now composed capabilities rather than name checks, so a command is
// hidden because a capability says so.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await page.waitForFunction(
    () => !!window.opencalcEditor && typeof window.opencalcEditor.listCommands === "function",
    null,
    { timeout: 30_000 },
  );
}

const caps = (page) => page.evaluate(() => window.opencalcEditor.getCapabilities());
const commands = (page) => page.evaluate(() => window.opencalcEditor.listCommands());

test("standalone is the default and forbids nothing", async ({ page }) => {
  await boot(page);
  const c = await caps(page);
  expect(c.mode).toBe("standalone");
  expect(c.canOpen && c.canSaveAs && c.canPrint).toBe(true);
  expect(c.ownsFile).toBeFalsy();

  const ids = await commands(page);
  expect(ids).toContain("file.open");
  expect(ids.filter((i) => /^file\.download/.test(i)).length).toBeGreaterThan(0);
});

test("an embedded editor cannot open or download the host's document", async ({ page }) => {
  await boot(page, "?mode=embedded");
  const c = await caps(page);
  expect(c.ownsFile).toBe(true);
  expect(c.canOpen).toBe(false);

  const ids = await commands(page);
  // The whole point: these are the host's, not the user's.
  expect(ids, "File ▸ Open must not exist when the host owns the file").not.toContain("file.open");
  expect(ids.filter((i) => /^file\.download/.test(i))).toEqual([]);
  // Print is the host's to allow, and it does.
  expect(ids).toContain("file.print");
});

test("a hidden command is not runnable either, so the API and the menu agree", async ({ page }) => {
  await boot(page, "?mode=embedded");
  // A command that is hidden but still runnable is a hole in the same wall:
  // the menu says no and the API says yes.
  const threw = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("file.open"); return false; }
    catch { return true; }
  });
  expect(threw).toBe(true);
});

test("ownsFile cannot be overridden back into opening", async ({ page }) => {
  await boot(page, "?mode=embedded");
  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canOpen: true }));
  // There is no host that means "the document is mine, and also let the user
  // swap it for another".
  expect((await caps(page)).canOpen).toBe(false);
  // But "download a copy" is a real permission a host may grant.
  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canSaveAs: true }));
  expect((await caps(page)).canSaveAs).toBe(true);
});

test("an unknown mode falls back to standalone, never to something restrictive", async ({ page }) => {
  await boot(page, "?mode=nonsense");
  const c = await caps(page);
  // A typo in a deployment's URL must not silently take a user's Save away.
  expect(c.mode).toBe("standalone");
  expect(c.canSaveAs).toBe(true);
});

test("chrome=native still means desktop, because the shell appends it", async ({ page }) => {
  await boot(page, "?chrome=native");
  expect((await caps(page)).chrome).toBe("native");
  await expect(page.locator("#menubar")).toBeHidden();
});

test("the file picker itself refuses, not just the menu entry", async ({ page }) => {
  await boot(page, "?mode=embedded");
  // The button can be hidden and the <input type=file> still sits in the host's
  // page. The gate belongs where a file becomes the document.
  const refused = await page.evaluate(() => window.opencalcEditor.capabilityForbids("file.open"));
  expect(refused).toBe(true);
});

test("running the command rules at boot does not reveal what was hidden", async ({ page }) => {
  await boot(page);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  // `applyCommandRules()` had never run at boot before. It wrote
  // `node.hidden = on` and `node.disabled = dim` outright, which asserts that
  // every command starts visible and enabled. Three do not, so simply calling
  // it would have revealed a bare "Choose file" control, revealed the toolbar's
  // overflow button, and enabled Undo over an empty history — three defects
  // introduced by a change that hides nothing.
  const state = await page.evaluate(() => ({
    open: document.getElementById("tb-open")?.hidden,
    more: document.getElementById("tb-more")?.hidden,
    undo: document.getElementById("tb-undo")?.disabled,
  }));
  expect(state.open, "the file input stays hidden").toBe(true);
  expect(state.more, "the toolbar overflow stays hidden at full width").toBe(true);
  expect(state.undo, "Undo stays disabled over an empty history").toBe(true);
});

test("a narrow window does not destroy the collapsed toolbar groups", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await boot(page);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  // The toolbar's collapse *moves* controls into a group's flyout, so every
  // `.tb-group` is genuinely empty on a small screen. Marking those groups
  // hidden is not recoverable — `expandGroup` sets `hidden = false`, which
  // cannot undo a `display: none !important` class — so the groups would have
  // gone for good on the next resize, and with no group left the whole toolbar
  // would have gone with them. Collapsed is not emptied.
  await expect(page.locator(".toolbar")).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.waitForTimeout(400);
  const live = await page.evaluate(
    () => [...document.querySelectorAll(".tb-group")].filter((g) => !g.hidden).length,
  );
  expect(live, "the groups come back when the window does").toBeGreaterThan(0);
});
