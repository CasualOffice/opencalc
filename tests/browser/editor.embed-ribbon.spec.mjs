// `<opencalc-sheet>` draws the Excel shell, not the legacy chrome.
//
// Reported by a user: the page editor had the ribbon and an embedded one still
// showed the old menu bar and toolbar. Three things kept it out, none of them
// visible from reading `embed.js` alone:
//
//  - `embed.js` keeps `editor.html`'s body and strips every `<script>`, and the
//    ribbon is started by an inline module at the bottom of that page;
//  - it adopted `editor.css` only, never `ribbon.tokens.css` / `ribbon.css`;
//  - `ribbon.js` reached for the page — `document.querySelector`, classes on
//    `<html>`, popovers on `document.body`, an icon sprite in the page that a
//    `<use href>` inside a shadow root cannot see, and a viewport media query
//    for the phone bar.
//
// So each claim here is measured inside the element's shadow root, and the
// event claims are measured against a host page with its own focusable input:
// an embedded editor answers its own keystrokes and nobody else's.

import { expect, test } from "@playwright/test";

/// A host page with an `<opencalc-sheet>` per entry of `sheets`.
async function host(page, sheets = [{ id: "a", width: 1000 }]) {
  const els = sheets
    .map((s) => `<opencalc-sheet id="${s.id}" style="display:block;height:${s.height ?? 520}px;width:${s.width}px"></opencalc-sheet>`)
    .join("\n");
  await page.route("**/__embed-ribbon.html", (route) => route.fulfill({
    contentType: "text/html",
    body: `<!doctype html><html><head><meta charset="utf-8"></head>
      <body style="margin:0">
        <input id="host-input" aria-label="the host's own input">
        ${els}
        <script type="module">import "/embed.js";</script>
      </body></html>`,
  }));
  await page.goto("/__embed-ribbon.html");
  for (const s of sheets) {
    await page.waitForFunction((id) => document.getElementById(id)?.ready?.then, s.id, { timeout: 30_000 });
    await page.evaluate((id) => document.getElementById(id).ready, s.id);
  }
}

const sheet = (page, id = "a") => page.locator(`opencalc-sheet#${id}`);

/// Whether A1 of the embed's active sheet is bold, asked of the engine.
const a1Bold = (page, id = "a") =>
  page.evaluate(async (id) => {
    const ed = await document.getElementById(id).ready;
    const f = JSON.parse(ed.wasmApi().session_cell_format(0, 0, 0));
    return !!f.b;
  }, id);

test("an embed draws the ribbon with its icons, and no legacy menu bar or toolbar", async ({ page }) => {
  await host(page);
  const s = sheet(page);
  await expect(s.locator(".oc-ribbon .rb-tabs"), "no ribbon tab row in the embed").toBeVisible();
  await expect(s.locator(".oc-ribbon .rb-band"), "no ribbon band in the embed").toBeVisible();
  await expect(s.locator("#rb-tab-home")).toBeVisible();

  const looked = await page.evaluate(() => {
    const root = document.getElementById("a").shadowRoot;
    const use = root.querySelector('.rb-band [data-cmd="toolbar.bold"] use');
    const id = use?.getAttribute("href")?.slice(1);
    const box = use?.getBBox();
    const onScreen = (sel) => {
      const n = root.querySelector(sel);
      if (!n) return false;
      const r = n.getBoundingClientRect();
      return getComputedStyle(n).display !== "none" && r.width > 0 && r.height > 0
        && r.right > 0 && r.left < innerWidth;
    };
    return {
      // `<use href>` resolves within its own tree: the symbol has to be in the
      // shadow root, not merely somewhere on the page.
      symbolInTree: !!(id && root.getElementById(id)?.tagName.toLowerCase() === "symbol"),
      drawn: box ? box.width > 0 && box.height > 0 : false,
      menubar: onScreen(".menubar"),
      toolbar: onScreen(".toolbar"),
    };
  });
  expect(looked.symbolInTree, "the Bold icon's symbol is not in the embed's tree").toBe(true);
  expect(looked.drawn, "the Bold icon draws nothing").toBe(true);
  expect(looked.menubar, "the legacy menu bar is on screen").toBe(false);
  expect(looked.toolbar, "the legacy toolbar is on screen").toBe(false);
});

test("a ribbon command runs in the embed, and its popovers open inside it, on screen", async ({ page }) => {
  await host(page);
  const s = sheet(page);
  expect(await a1Bold(page)).toBe(false);
  const bold = s.locator('.oc-ribbon [data-cmd="toolbar.bold"]');
  await bold.click();
  await expect.poll(() => a1Bold(page), { message: "Bold in the embed's ribbon did not bold A1" }).toBe(true);
  await expect(bold).toHaveAttribute("aria-pressed", "true");

  // The shell's own popover (Ribbon Display Options), and a toolbar control's
  // popup re-anchored under the ribbon button (Borders).
  const placed = async (trigger, popSel) => {
    await s.locator(trigger).click();
    return page.evaluate(([trigger, popSel]) => {
      const root = document.getElementById("a").shadowRoot;
      const pop = [...root.querySelectorAll(popSel)].find((p) => !p.hidden && p.getBoundingClientRect().height > 0);
      if (!pop) return { found: false, inDocument: !!document.querySelector(popSel) };
      const r = pop.getBoundingClientRect();
      const b = root.querySelector(trigger).getBoundingClientRect();
      return {
        found: true,
        onScreen: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
        // Below its button, or above it when there is no room: next to it.
        nearTrigger: Math.abs(r.top - b.bottom) < 8 || Math.abs(r.bottom - b.top) < 8,
        styled: getComputedStyle(pop).position === "fixed",
      };
    }, [trigger, popSel]);
  };
  const display = await placed(".oc-ribbon .rb-layout", ".rb-pop");
  expect(display, "the ribbon's own menu did not open inside the embed").toMatchObject({ found: true, onScreen: true, nearTrigger: true, styled: true });
  // A click on the popover itself, between its items, is not an outside
  // click. From the page's listener every click in a shadow root looks like a
  // click on the host element, which the popover does not contain.
  await s.locator(".rb-pop").click({ position: { x: 3, y: 3 } });
  await expect(s.locator(".rb-pop"), "a click inside the popover closed it").toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(s.locator(".rb-pop")).toHaveCount(0);

  const borders = await placed('.oc-ribbon [data-cmd="toolbar.border"]', ".toolbar .popmenu");
  expect(borders, "the Borders popup did not open under its ribbon button").toMatchObject({ found: true, onScreen: true, nearTrigger: true });
  await page.keyboard.press("Escape");

  // The File page covers the editor it belongs to, not the host's page.
  await s.locator(".oc-ribbon .rb-file").click();
  const backstage = s.locator(".rb-backstage");
  await expect(backstage).toBeVisible();
  const fit = await page.evaluate(() => {
    const el = document.getElementById("a");
    const bs = el.shadowRoot.querySelector(".rb-backstage").getBoundingClientRect();
    const box = el.getBoundingClientRect();
    return { left: bs.left - box.left, top: bs.top - box.top, width: bs.width - box.width, height: bs.height - box.height };
  });
  expect(fit, "the File page is not the element's size and place").toEqual({ left: 0, top: 0, width: 0, height: 0 });
  await page.keyboard.press("Escape");
  await expect(backstage).toBeHidden();
});

test("an embed answers its own keystrokes, not the host page's", async ({ page }) => {
  await host(page);
  const s = sheet(page);
  const tips = s.locator(".oc-ribbon.rb-keytips");
  const band = s.locator(".oc-ribbon .rb-band");

  // Focus in the host's own input: Alt and Ctrl+F1 are the host's.
  await page.locator("#host-input").focus();
  await page.keyboard.press("Alt");
  await expect(tips, "Alt on the host page raised the embed's KeyTips").toHaveCount(0);
  await page.keyboard.press("Control+F1");
  await expect(band, "Ctrl+F1 on the host page collapsed the embed's ribbon").toBeVisible();

  // Focus in the embed: they are the embed's.
  await s.locator("#grid").focus();
  await page.keyboard.press("Alt");
  await expect(tips, "Alt in the embed raised no KeyTips").toHaveCount(1);
  // The host's Escape is not ours either.
  await page.locator("#host-input").focus();
  await page.keyboard.press("Escape");
  await expect(tips, "Escape on the host page reached the embed").toHaveCount(1);
  await s.locator("#grid").focus();
  await page.keyboard.press("Escape");
  await expect(tips).toHaveCount(0);
});

// **Two embeds on one page — not reachable yet, and not because of the shell.**
//
// Measured on `main` before this change, with no shell involved at all: the
// second element's `start()` throws `Cannot read properties of undefined
// (reading 'default_col_px')` and the first is left at "loading engine…".
// `embed.js` imports `editor.js?…&i=<key>` to get a module scope per element,
// but since `MNT-005` split the editor, `editor.js` is a re-export of
// `editor.core.js`, imported under one URL for every element — so all of them
// share one editor. The shell is built per root (`ribbon.js` `createShell`) and
// its events are scoped by `composedPath()`, so this test is written to the
// promise and switched on when the editor itself can be mounted twice.
test.fixme("two embeds on one page: a command and KeyTips in one leave the other alone", async ({ page }) => {
  await host(page, [{ id: "a", width: 900, height: 360 }, { id: "b", width: 900, height: 360 }]);
  await sheet(page, "a").locator('.oc-ribbon [data-cmd="toolbar.bold"]').click();
  await expect.poll(() => a1Bold(page, "a")).toBe(true);
  expect(await a1Bold(page, "b"), "Bold in one embed bolded the other").toBe(false);

  await sheet(page, "a").locator("#grid").focus();
  await page.keyboard.press("Alt");
  await expect(sheet(page, "a").locator(".oc-ribbon.rb-keytips")).toHaveCount(1);
  await expect(sheet(page, "b").locator(".oc-ribbon.rb-keytips"), "one Alt raised KeyTips in both").toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(sheet(page, "a").locator(".oc-ribbon.rb-keytips")).toHaveCount(0);
});

test("chrome regions map onto the shell: toolbar is the band, menubar the tab row, preview neither", async ({ page }) => {
  await host(page);
  const s = sheet(page);
  const ribbonHeight = () => page.evaluate(() =>
    document.getElementById("a").shadowRoot.querySelector(".oc-ribbon").getBoundingClientRect().height);
  await expect(s.locator(".oc-ribbon .rb-band")).toBeVisible();

  await page.evaluate(() => document.getElementById("a").chrome({ toolbar: false }));
  await expect(s.locator(".oc-ribbon .rb-band"), "toolbar:false left the band").toBeHidden();
  await expect(s.locator("#rb-tab-home"), "toolbar:false took the tabs too").toBeVisible();

  await page.evaluate(() => document.getElementById("a").chrome({ menubar: false, toolbar: false }));
  await expect(s.locator("#rb-tab-home"), "menubar:false left the tab row").toBeHidden();
  await expect(s.locator(".oc-ribbon .rb-file")).toBeHidden();
  expect(await ribbonHeight(), "with both off there is still a ribbon").toBe(0);
  // Who else is here is not chrome (`UX-CHROME-01`): with the title bar, the
  // tab row and the band all off, a co-editor is still on screen.
  const roster = await page.evaluate(() => {
    const presence = document.getElementById("a").shadowRoot.getElementById("presence");
    presence.removeAttribute("hidden");
    const r = presence.getBoundingClientRect();
    const seen = getComputedStyle(presence).display !== "none" && r.width > 0 && r.height > 0;
    presence.setAttribute("hidden", "");
    return { seen, inTabRow: !!presence.closest(".rb-tabs") };
  });
  expect(roster, "hiding the ribbon took the roster with it").toEqual({ seen: true, inTabRow: true });

  await page.evaluate(() => document.getElementById("a").chrome({ menubar: true, toolbar: true }));
  await expect(s.locator(".oc-ribbon .rb-band")).toBeVisible();
  await page.evaluate(() => document.getElementById("a").configure({ access: "preview" }));
  await expect(s.locator("#rb-tab-home"), "a preview shows the ribbon's tabs").toBeHidden();
  expect(await ribbonHeight(), "a preview shows a ribbon").toBe(0);

  await page.evaluate(() => document.getElementById("a").configure({ access: "edit" }));
  await expect(s.locator(".oc-ribbon .rb-band"), "leaving preview did not bring the ribbon back").toBeVisible();
});

test("view access keeps the ribbon and takes the editing commands off it", async ({ page }) => {
  await host(page);
  const s = sheet(page);
  const bold = s.locator('.oc-ribbon [data-cmd="toolbar.bold"]');
  await expect(bold).toBeEnabled();
  await page.evaluate(() => document.getElementById("a").configure({ access: "view" }));
  await expect(s.locator(".oc-ribbon .rb-band"), "a viewer lost the ribbon, which is preview's job").toBeVisible();
  // The command policy decides; the ribbon shows what it says, either way.
  await expect.poll(() => bold.evaluate((b) => b.hidden || b.disabled), { message: "Bold is still live for a viewer" }).toBe(true);
  await page.evaluate(() => document.getElementById("a").configure({ access: "edit" }));
  await expect(bold).toBeEnabled();
});

test("a narrow embed in a wide window uses the phone bar", async ({ page }) => {
  // The window is Playwright's desktop 1280px; only the element is narrow.
  await host(page, [{ id: "a", width: 400, height: 600 }]);
  const s = sheet(page);
  await expect(s.locator(".rb-mobile"), "a 400px embed has no phone bar").toBeVisible();
  await expect(s.locator(".oc-ribbon .rb-band"), "a 400px embed drew the desktop band").toBeHidden();
  // A phone bar that is there but empty is not a phone bar.
  expect(await s.locator(".rb-micons .rb-mbtn").count()).toBeGreaterThan(3);
  // And it follows the element: widened by its host, it is a desktop again.
  await page.evaluate(() => { document.getElementById("a").style.width = "1000px"; });
  await expect(s.locator(".oc-ribbon .rb-band"), "widening the element did not bring the band back").toBeVisible();
  await expect(s.locator(".rb-mobile")).toBeHidden();
});
