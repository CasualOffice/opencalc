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
  // The desktop *metrics* follow from the mode alone, which is what this test
  // is about: `?chrome=native` is a URL contract `desktop/src/main.rs` already
  // ships and it still resolves to the `desktop` preset.
  await expect(page.locator(".app-header")).toBeHidden();

  // **This used to assert `#menubar` was hidden, and that was the defect**
  // (`UX-CHR-02`). Hiding the menu bar takes the File and View menus away, and
  // in a browser nothing draws any in their place — a user was left with no
  // menus and no route to the theme. The bar now waits for evidence that a
  // native one exists, which a query string is not, so it is *visible* here.
  // The evidence, and the reclaim it still triggers in the shell, are asserted
  // in `editor.native-chrome.spec.mjs`, which can install that evidence.
  await expect(page.locator("#menubar"), "a browser has no other menu to fall back on")
    .toBeVisible();
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

// --- An embed that says nothing (`UX-EMBED-02`) ------------------------------
//
// Everything above drives the mode from `?mode=`, and every real embedding sets
// no `?mode=` at all. Measured before the fix, against this same server:
//
//   | mount                        | mode       | canOpen | canSaveAs | canShare | ownsFile | chrome   | forbidden ids listed |
//   |------------------------------|------------|---------|-----------|----------|----------|----------|----------------------|
//   | editor.html (page)           | standalone | true    | true      | true     | false    | web      | 13                   |
//   | editor.html?mode=embedded    | embedded   | false   | false     | false    | true     | embedded | 0                    |
//   | framed editor.html           | standalone | true    | true      | true     | false    | web      | 13                   |
//   | <opencalc-sheet> shadow root | standalone | true    | true      | true     | false    | web      | 13                   |
//
// So both real embeddings were byte-identical to our own page: `File ▸ New` and
// `File ▸ Open` listed *and runnable*, so a visitor could replace the host's
// document from inside the host's own page; eight download entries the host
// never authorised; `File ▸ Share…` in somebody else's product, which the
// `embedded` preset says in as many words must be the host's decision; and our
// branding strip inside their page.
//
// The two mounts need two different fixes, which is why both are tested here. A
// frame is decidable while the module is still evaluating, which it has to be —
// `capabilityModeName` is initialised at module scope. A shadow root is not:
// `setMountRoot()` runs afterwards, so the mode is corrected there.

/// An `editor.html` inside an iframe, which is how `casual-calc-host`, the WOPI
/// adapter and the landing page all embed it. `docs.html` is a host page that is
/// not itself the editor, so the only editor on the origin is the framed one.
async function framedEditor(page, query = "") {
  await page.goto("/docs.html");
  await page.evaluate((q) => {
    const f = document.createElement("iframe");
    f.id = "oc-frame";
    f.src = `/editor.html${q}`;
    f.style.cssText = "width:900px;height:600px";
    document.body.append(f);
  }, query);
  await expect
    .poll(
      async () => {
        const f = page.frames().find((fr) => fr.url().includes("/editor.html"));
        if (!f) return false;
        return f
          .evaluate(() => typeof window.opencalcEditor?.listCommands === "function")
          .catch(() => false);
      },
      { timeout: 30_000 },
    )
    .toBe(true);
  return page.frames().find((fr) => fr.url().includes("/editor.html"));
}

/// An `<opencalc-sheet>`, which mounts the same editor into a shadow root.
async function shadowEditor(page, query = "") {
  await page.goto(`/embed.html${query}`);
  await page.waitForFunction(
    () => document.querySelector("opencalc-sheet#sheet")?.ready?.then,
    null,
    { timeout: 30_000 },
  );
  await page.evaluate(() => document.querySelector("opencalc-sheet#sheet").ready);
}

/// Read the whole answer at once, so a mount is described rather than sampled.
///
/// The count of ids is deliberately not asserted anywhere below: the download
/// submenu has grown from six entries to eight since this table was first
/// measured, and a test that asserts "13" fails for the wrong reason the next
/// time a format is added.
const profileOf = (target) =>
  target.evaluate(() => {
    const hostsOnly = /^file\.new$|^file\.open$|^header\.open$|^toolbar\.open$|^file\.download|^file\.share$/;
    const ed = window.opencalcEditor;
    const ids = ed.listCommands();
    let ran = "ran";
    try { ed.runCommand("file.open"); } catch (e) { ran = `refused: ${e.message}`; }
    return {
      caps: ed.getCapabilities(),
      listed: ids.filter((i) => hostsOnly.test(i)),
      forbids: ed.capabilityForbids("file.open"),
      ran,
    };
  });

test("a framed editor.html defaults to embedded, and File ▸ Open is gone from it", async ({ page }) => {
  const frame = await framedEditor(page);
  const p = await profileOf(frame);

  expect(p.caps.mode, "an embed that says nothing is still an embed").toBe("embedded");
  expect(p.caps.ownsFile).toBe(true);
  expect(p.caps.canOpen).toBe(false);
  expect(p.caps.canSaveAs, "the host never authorised a download").toBe(false);
  expect(
    p.caps.canShare,
    "starting a session in somebody else's product is theirs to decide",
  ).toBe(false);
  expect(p.caps.chrome, "our branding strip is duplication inside their page").toBe("embedded");

  // The flags are not the point — a hardcoded `false` would satisfy them. These
  // three are the mechanism: nothing lists it, the gate refuses it, and the API
  // refuses it too, so the menu and a script agree.
  expect(p.listed, "an embed offered the host's document to a visitor").toEqual([]);
  expect(p.forbids).toBe(true);
  expect(p.ran).toMatch(/^refused:/);
});

test("an <opencalc-sheet> defaults to embedded, though its mode is settled after the module loads", async ({ page }) => {
  await shadowEditor(page);
  // `window.opencalcEditor` is the element's own module instance — `main()` sets
  // it — and the only editor on this page is the embedded one.
  const p = await profileOf(page);

  expect(p.caps.mode, "setMountRoot is the only place this case can be caught").toBe("embedded");
  expect(p.caps.ownsFile).toBe(true);
  expect(p.caps.canOpen).toBe(false);
  expect(p.caps.canSaveAs).toBe(false);
  expect(p.caps.canShare).toBe(false);
  expect(p.listed, "a shadow-root embed offered the host's document to a visitor").toEqual([]);
  expect(p.forbids).toBe(true);
  expect(p.ran).toMatch(/^refused:/);
});

/// **The default moved; the ceiling did not.**
///
/// Somebody deliberately embedding a full editor — an intranet host that owns
/// nothing, a demo — must still be able to ask for one. If this ever fails, the
/// change above stopped being a default and became a prohibition.
test("an explicit ?mode= still wins inside a frame", async ({ page }) => {
  const frame = await framedEditor(page, "?mode=standalone");
  const p = await profileOf(frame);

  expect(p.caps.mode).toBe("standalone");
  expect(p.caps.canOpen).toBe(true);
  expect(p.caps.ownsFile).toBeFalsy();
  expect(p.listed, "the wider set was asked for and must be there").toContain("file.open");
  expect(p.forbids).toBe(false);
});

/// The same rule at the other site. `setMountRoot()` is a second place a default
/// is chosen, so it is a second place a default could overwrite a decision, and
/// the two are only guarded by the same predicate for as long as somebody keeps
/// checking.
test("an explicit ?mode= still wins in a shadow root", async ({ page }) => {
  await shadowEditor(page, "?mode=standalone");
  const p = await profileOf(page);

  expect(p.caps.mode).toBe("standalone");
  expect(p.caps.canOpen).toBe(true);
  expect(p.listed).toContain("file.open");
  expect(p.forbids).toBe(false);
});

/// **The embedded chrome stays inside the embed.**
///
/// `oc-chrome-embedded` used to go on `document.documentElement`, which was
/// harmless only while a shadow-root mount could never reach `chrome:
/// "embedded"`. Now that it can, that `<html>` is the *host's*: the class would
/// cross the shadow boundary in the one direction the boundary exists to
/// prevent, and `.oc-chrome-embedded .app-header` would hide a header belonging
/// to the host.
test("an embed's chrome class does not escape onto the host page", async ({ page }) => {
  await shadowEditor(page);
  const where = await page.evaluate(() => ({
    host: document.documentElement.classList.contains("oc-chrome-embedded"),
    inside: !!document
      .querySelector("opencalc-sheet#sheet")
      .shadowRoot.querySelector(".editor-body.oc-chrome-embedded"),
  }));
  expect(where.host, "the host page's own <html> was restyled by its guest").toBe(false);
  expect(where.inside, "the embed still marks its own chrome").toBe(true);
});
