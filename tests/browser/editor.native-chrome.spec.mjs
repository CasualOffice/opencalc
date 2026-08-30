// A desktop app should behave like a desktop app.
//
// The seam that makes that possible without a second menu definition. The
// editor already funnels every menu item and toolbar button through one command
// id (`data-oc-command`, `listCommands()`, `runCommand(id)`), so a native menu
// does not need its own copy of the File/Edit/View tree — it needs the same
// tree in a form a native menu builder can read, and a way to turn the HTML bar
// off when the operating system is drawing one instead.
//
// The model is derived from the **live DOM** rather than from the `MENUS`
// literal it is built from. That is deliberate: the DOM is what `runCommand`
// dispatches against, and `applyCommandRules()` hides items in read-only mode,
// so a DOM-derived model cannot drift from what is actually clickable. A model
// read from the literal would describe a menu the app might not have.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const model = (page) => page.evaluate(() => window.opencalcEditor.menuModel());

test("the menu model describes the whole menu bar", async ({ page }) => {
  await boot(page);
  const menus = await model(page);

  expect(menus.length, "eight top-level menus").toBeGreaterThanOrEqual(8);
  const names = menus.map((m) => m.label);
  expect(names).toContain("File");
  expect(names).toContain("Help");

  const file = menus.find((m) => m.label === "File");
  expect(file.id, "the id a native menu dispatches with").toBe("file");
  expect(file.items.length, "File has items").toBeGreaterThan(3);

  // Separators are carried, because a native menu without them is a wall.
  const anySep = menus.some((m) => m.items.some((i) => i.kind === "separator"));
  expect(anySep, "separators survive into the model").toBe(true);

  // Submenus stay nested rather than being flattened.
  const anySub = menus.some((m) => m.items.some((i) => i.kind === "submenu" && i.items.length));
  expect(anySub, "submenus stay nested").toBe(true);
});

test("every command in the model can be dispatched", async ({ page }) => {
  await boot(page);
  const menus = await model(page);
  const ids = await page.evaluate(() => window.opencalcEditor.listCommands());

  const leaves = [];
  const walk = (items) => {
    for (const i of items) {
      if (i.kind === "submenu") walk(i.items);
      else if (i.kind === "item") leaves.push(i);
    }
  };
  for (const m of menus) walk(m.items);

  expect(leaves.length, "the model has leaves").toBeGreaterThan(30);
  // The whole point: a native menu holds these ids and calls runCommand with
  // them. Any id the model invents is a menu entry that does nothing.
  const unknown = leaves.map((l) => l.id).filter((id) => !ids.includes(id));
  expect(unknown, "no id in the model is unknown to runCommand").toEqual([]);
});

test("chrome=native hides the HTML bar and gives the height to the grid", async ({ page }) => {
  await boot(page);
  const webGrid = await page.locator("#grid").boundingBox();
  const barHeight = await page.evaluate(() => document.getElementById("menubar").getBoundingClientRect().height);
  expect(barHeight, "there is a bar to reclaim").toBeGreaterThan(20);

  await boot(page, "?chrome=native");
  await expect(page.locator("#menubar")).toBeHidden();
  const nativeGrid = await page.locator("#grid").boundingBox();

  // Not merely hidden — the space it occupied goes to the sheet, which is the
  // only reason a desktop app hiding its own menu bar is an improvement.
  expect(nativeGrid.height, "the grid grows by roughly the bar").toBeGreaterThan(webGrid.height + barHeight - 4);
});

test("commands still run when the operating system owns the menu", async ({ page }) => {
  await boot(page, "?chrome=native");
  await expect(page.locator("#menubar")).toBeHidden();

  // The native menu dispatches by id into a bar the user cannot see. If hiding
  // the bar detached or disabled its nodes, every native menu entry would throw
  // and the desktop app would have a menu that does nothing.
  const ran = await page.evaluate(() => window.opencalcEditor.runCommand("file.properties"));
  expect(ran).toBe(true);
  await expect(page.locator(".oc-modal:not([hidden])")).toBeVisible();
});


// --- The window is not a web page (`UX-DESK-01`) ------------------------------
//
// Reported on first sight of the desktop build: *"UI and desktop experience
// still looks like a web application, instead of a real desktop application
// like OnlyOffice or Excel or LibreOffice."*
//
// Everything above this line is about the *menu*. What was left is the strip
// above it: a logo, "OpenCalc", an *Alpha* badge and "engine v0.0.0" — 53px of
// branding sitting where a desktop application has nothing at all, directly
// under a title bar that already reads `figures.xlsx — OpenCalc`. Not one of
// the three applications the report names puts a logo or a product name inside
// the document window.
//
// **It was never a one-line hide, and that is what these tests are for.**
// `#settings-panel` lived *inside* `.app-header`, so `display: none` on the
// header took Settings with it and `Tools > Settings...` in the operating
// system's own menu would have opened a panel inside a hidden ancestor — a menu
// entry that appears to do nothing. Two more nodes had the same shape:
// `#tb-status` carries every message the editor reports, and `#presence` is the
// collaborator roster `COL-33` deliberately kept out of the header so it would
// not fold away with it — only for `.oc-chrome-native #menubar { display: none }`
// to fold it away instead.
//
// So each of these asserts on *computed style and geometry*, never on a class
// being present. "The class is applied" is not the promise; "the user can see
// it and use it" is.

/// Where a node is, and whether it is genuinely on screen — not merely attached.
const seen = (page, selector) =>
  page.evaluate((sel) => {
    const el = document.querySelector(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return {
      parent: el.parentElement?.className ?? null,
      inBottomBar: !!el.closest(".bottom-bar"),
      painted: s.display !== "none" && s.visibility !== "hidden" && r.width > 0 && r.height > 0,
      box: { x: r.x, y: r.y, w: r.width, h: r.height },
      onScreen: r.left >= 0 && r.top >= 0 && r.right <= window.innerWidth && r.bottom <= window.innerHeight,
    };
  }, selector);

/// **The trap the row names, in one test.**
///
/// The header goes *and* Settings still opens *and* it is still usable. Any one
/// of the three alone is the defect: a header that stays is the complaint, a
/// panel that opens inside a hidden ancestor is a menu item that does nothing,
/// and a panel that is on screen but whose controls are inert is worse than
/// either, because it looks like it worked.
test("the branding strip is gone and Settings still opens from the menu", async ({ page }) => {
  await boot(page, "?chrome=native");

  await expect(page.locator(".app-header"), "a desktop window has no branding strip").toBeHidden();

  // Reached the way the operating system's menu reaches it: by command id, with
  // no gear on screen to click.
  const ran = await page.evaluate(() => window.opencalcEditor.runCommand("view.settings"));
  expect(ran, "the menu's own command id still dispatches").toBe(true);

  const panel = await seen(page, "#settings-panel");
  expect(panel.painted, "Settings opened somewhere the user can see it").toBe(true);
  expect(panel.onScreen, "and inside the window, not off an edge").toBe(true);
  expect(panel.parent, "it no longer lives inside the header that would hide it").not.toBe("settings");

  // Usable, not merely visible. Applying a theme is the panel's whole point.
  await page.selectOption("#set-theme", "dark");
  await expect
    .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
    .toBe("dark");
});

/// The same panel, in the page, must not have moved to look at.
///
/// Taking it out of the header is what made the desktop case possible; it must
/// not have cost the web page its popover. The assertion that matters is
/// geometric rather than a class: it hangs under the gear.
test("in the page, Settings is still a popover under the gear", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("tools.settings"));

  const panel = await seen(page, "#settings-panel");
  const gear = await page.locator("#tb-settings").boundingBox();
  expect(panel.painted).toBe(true);
  expect(panel.onScreen).toBe(true);
  expect(panel.box.y, "below the gear").toBeGreaterThan(gear.y + gear.height - 1);
  expect(Math.abs(panel.box.x + panel.box.w - (gear.x + gear.width)), "right-aligned to it").toBeLessThan(2);
});

/// **The status line is relocated, not hidden.**
///
/// It is the engine version, the open/save progress line and every error the
/// editor reports. Excel, LibreOffice and OnlyOffice all put document state in
/// the status bar, and none of them puts any of it above the menu bar.
test("the status line moves into the status bar rather than going with the header", async ({ page }) => {
  await boot(page, "?chrome=native");

  const status = await seen(page, "#tb-status");
  expect(status.inBottomBar, "the status line is in the status bar").toBe(true);
  expect(status.painted, "and is actually on screen there").toBe(true);
});

/// **The roster survives the menu bar being hidden** — `COL-33`'s rule, applied
/// to the one chrome that had broken it. `desktop` has `canShare: true`, so this
/// was a session whose participants the desktop user could not see.
test("the collaborator roster is still visible when the OS draws the menu", async ({ page }) => {
  await boot(page, "?chrome=native");
  // The roster carries `hidden` until there is a session; that is the state the
  // question is about, not the state being asserted.
  await page.evaluate(() => document.getElementById("presence").removeAttribute("hidden"));

  const roster = await seen(page, "#presence");
  expect(roster.inBottomBar, "it is not inside the hidden menu bar").toBe(true);
  expect(roster.painted, "and it can be seen").toBe(true);

  // **Beside the status line, not stranded in the middle of the bar.**
  //
  // `.presence` carries `margin-left: auto` for the menu bar, and the note on
  // that rule says what two auto margins in one flex row do — the free space is
  // split and the roster parks itself mid-bar. It did: x=601 in a 1280px status
  // bar, adjacent to nothing, while `inBottomBar` and `painted` were both still
  // true. Adjacency is the part an assertion has to carry, because "visible" did
  // not notice.
  const status = await seen(page, "#tb-status");
  expect(roster.box.x - (status.box.x + status.box.w), "the roster sits beside the status line")
    .toBeLessThan(40);
});

/// **A native toolbar is tighter than a web one.**
///
/// LibreOffice's toolbar icons are 16/24/32 with 24 the default, and a platform
/// menu bar is ~20-24px; the one tall band in the set, Excel's ribbon, ships
/// with three documented ways to collapse it. Asserted as a comparison rather
/// than against pixel constants, so a redesign of the page's own metrics is not
/// a false failure here — the invariant is that the desktop is denser, not that
/// it is 36px.
test("desktop chrome is denser than the page's, and the sheet gets the difference", async ({ page }) => {
  const bands = () =>
    page.evaluate(() => {
      const h = (sel) => document.querySelector(sel).getBoundingClientRect().height;
      return { toolbar: h(".toolbar"), formula: h(".formula-bar"), bottom: h(".bottom-bar"), grid: h("#grid") };
    });

  await boot(page);
  const web = await bands();
  await boot(page, "?chrome=native");
  const native = await bands();

  expect(native.toolbar, "the toolbar is tighter").toBeLessThan(web.toolbar);
  expect(native.formula, "so is the formula bar").toBeLessThan(web.formula);
  expect(native.bottom, "so is the status bar").toBeLessThan(web.bottom);
  // The header (53px) and the menu bar (30px), plus the band savings. Asserted
  // loosely, because the exact number is a metric and the claim is not.
  expect(native.grid - web.grid, "and every pixel of it goes to the sheet").toBeGreaterThan(90);
});

/// **A mode can be turned off again.**
///
/// `setCapabilities({ mode })` is a host surface, so a relocation has to be
/// reversible; a one-way move would leave the page's own header permanently
/// missing its status line the moment anything switched modes.
test("leaving desktop chrome puts the moved nodes back", async ({ page }) => {
  await boot(page, "?chrome=native");
  expect((await seen(page, "#tb-status")).inBottomBar).toBe(true);

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ mode: "standalone" }));
  await expect(page.locator(".app-header")).toBeVisible();

  const status = await seen(page, "#tb-status");
  expect(status.inBottomBar, "the status line went home").toBe(false);
  expect(status.painted, "and is on screen in the header").toBe(true);
  expect(
    await page.evaluate(() => !!document.querySelector("#menubar #presence")),
    "so did the roster",
  ).toBe(true);
});
