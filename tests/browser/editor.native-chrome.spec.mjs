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

/// **A desktop shell, as far as the page can tell** (`UX-CHR-02`).
///
/// The gap this closes is the reason `UX-CHR-02` shipped at all. Every test in
/// this file runs in a browser, where `?chrome=native` changes some CSS and no
/// operating system draws anything — so a rule that hid the HTML menu bar *on
/// the strength of the query string* looked correct here and stranded a real
/// user, with no File menu, no View menu and no route to the theme. Nothing in
/// the suite could observe the difference, because nothing in the suite could
/// say "and a native menu exists".
///
/// `window.__opencalcNative` is what says it. `desktop/src/main.rs`'s
/// `BOOTSTRAP` installs exactly this object and publishes the menu in the same
/// breath, and `applyCommandRules()` already calls `publishMenu()` on it — so
/// its presence *is* the native bar, in the product as well as here. Installed
/// with `addInitScript` so it is there before the editor's own modules
/// evaluate, which is where the shell's `withGlobalTauri` global would be.
///
/// Every function the editor may call is stubbed, not just the ones a given
/// test needs: `#tb-open` is intercepted through `native.open`, the title
/// poller calls `native.setDocument` every 250ms, and a missing one of those
/// throws inside a timer where no assertion would ever see it.
const installShell = (page) =>
  page.addInitScript(() => {
    window.__opencalcNativeCalls = [];
    const record = (fn) => (...args) => {
      window.__opencalcNativeCalls.push({ fn, args: args.length });
      return Promise.resolve(null);
    };
    window.__opencalcNative = {
      open: record("open"),
      save: record("save"),
      saveTarget: record("saveTarget"),
      clearSaveTarget: record("clearSaveTarget"),
      setDocument: record("setDocument"),
      syncCapabilities: record("syncCapabilities"),
      publishMenu: record("publishMenu"),
    };
  });

/// The editor as the desktop shell actually presents it: the query string the
/// shell's `tauri.conf.json` navigates to, *and* the bridge it injects.
async function bootShell(page, query = "?chrome=native") {
  await installShell(page);
  await boot(page, query);
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

test("the shell's chrome hides the HTML bar and gives the height to the grid", async ({ page }) => {
  await boot(page);
  const webGrid = await page.locator("#grid").boundingBox();
  const barHeight = await page.evaluate(() => document.getElementById("menubar").getBoundingClientRect().height);
  expect(barHeight, "there is a bar to reclaim").toBeGreaterThan(20);

  await bootShell(page);
  await expect(page.locator("#menubar")).toBeHidden();
  const nativeGrid = await page.locator("#grid").boundingBox();

  // Not merely hidden — the space it occupied goes to the sheet, which is the
  // only reason a desktop app hiding its own menu bar is an improvement.
  // `UX-DESK-01`'s reclaim is not up for reversal by `UX-CHR-02`; what changed
  // is who is allowed to trigger it.
  expect(nativeGrid.height, "the grid grows by roughly the bar").toBeGreaterThan(webGrid.height + barHeight - 4);
});

test("commands still run when the operating system owns the menu", async ({ page }) => {
  await bootShell(page);
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

  // Usable, not merely visible. The theme control this used to drive has moved
  // to `View ▸ Theme` (`UX-CHR-01`), so the panel is exercised through a
  // setting it still owns — the claim is that a dialog opened from a menu with
  // no gear on screen is a working dialog, not that any one control is in it.
  await page.locator('#set-accent button[data-c="#16a34a"]').click();
  await expect
    .poll(() => page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--oc-accent-color").trim()))
    .toBe("#16a34a");
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

/// **One toolbar, drawn at one set of metrics — and tighter is not smaller.**
///
/// LibreOffice's toolbar icons are 16/24/32 with 24 the default, and a platform
/// menu bar is ~20-24px; the one tall band in the set, Excel's ribbon, ships
/// with three documented ways to collapse it.
///
/// **This test used to assert the opposite of what it asserts now, and the
/// reversal is the point.** It read `native < web`, band for band, because
/// there were two metric sets and the desktop's was the tight one. That is the
/// defect `UX-CHR-05` closed: two sets drift, and these had — a `height: 20px`
/// on the roster chip and a 24px sheet tab each made a *desktop* control
/// smaller than the page's own, which is the opposite of density and was
/// invisible from either side alone. The desktop numbers were the measured
/// ones, so they became the only ones; the claim is now that the two chromes
/// measure equal, and that what a desktop window gains is the two *regions* it
/// does not draw.
///
/// **Both ends are still pinned, because only one end used to be.** The first
/// cut of this asserted `native < web` and `grid > web + 90` and nothing else,
/// so it was satisfied by a toolbar of any size at all down to zero — and the
/// metric it passed on put 26px buttons in a desktop window, smaller than
/// LibreOffice (~26-30), well under OnlyOffice (32-36), and under the ~28px
/// mark where a mouse target starts needing aim. That floor is absolute now
/// rather than relative to the page's own control, which is what it always
/// claimed to be and what a single metric set forces it to say out loud.
const BANDS = [".toolbar", ".formula-bar", ".bottom-bar"];

/// Controls the native metrics resize, and the text they carry. Measured by
/// selector in both chromes and compared element for element.
const NATIVE_CONTROLS = [
  ".toolbar .tb-btn",
  ".toolbar .tb-select",
  ".toolbar input.tb-font",
  ".toolbar input.tb-size",
  "#formula-input",
  "#cell-ref",
  ".sheet-tab",
];
const NATIVE_TEXT = [
  "#formula-input", "#cell-ref", ".sheet-tab", "#tb-status",
  ".toolbar .tb-select", ".toolbar input.tb-font", ".toolbar input.tb-size", "#zoom-level",
];

test("the desktop and the page share one metric set, and the sheet gets the regions", async ({ page }) => {
  // Wide enough that no toolbar group has collapsed into a flyout, or half
  // these controls measure 0x0 because they are not on the bar at all.
  await page.setViewportSize({ width: 1600, height: 900 });

  const metrics = () =>
    page.evaluate(({ bands, controls, text }) => {
      const h = (sel) => document.querySelector(sel).getBoundingClientRect().height;
      const box = {};
      for (const sel of controls) {
        const el = document.querySelector(sel);
        const r = el?.getBoundingClientRect();
        if (r && r.height > 0) box[sel] = { w: r.width, h: r.height };
      }
      const type = {};
      for (const sel of text) {
        const el = document.querySelector(sel);
        if (el) type[sel] = parseFloat(getComputedStyle(el).fontSize);
      }
      return {
        band: Object.fromEntries(bands.map((s) => [s, h(s)])),
        // The two regions desktop chrome does not draw, measured wherever this
        // runs — which is why it is part of `metrics()` and not a second
        // evaluate afterwards: read after the shell has booted, both are 0 and
        // the assertion below compares 84 against nothing.
        region: { header: h(".app-header"), menubar: h("#menubar") },
        grid: h("#grid"),
        box,
        type,
      };
    }, { bands: BANDS, controls: NATIVE_CONTROLS, text: NATIVE_TEXT });

  await boot(page);
  const web = await metrics();
  await bootShell(page);
  const native = await metrics();

  // --- Dense, and now dense in **both** chromes (`UX-CHR-05`). --------------
  //
  // This asserted `native < web`, band for band, and it was right for as long
  // as there were two metric sets. There are not: the desktop numbers were the
  // measured ones — argued against LibreOffice, OnlyOffice and Excel and
  // corrected upward once for having gone below all three — and the web set was
  // simply the loose one, so `UX-CHR-05` moved the measured numbers into the
  // base rules and deleted `.oc-chrome-native`'s metric block. Equality is
  // therefore the stronger claim, not a weaker one: `UX-DESK-01`'s density is
  // kept *and* the page gets it too, and neither chrome can drift from the
  // other by a number nobody is comparing.
  for (const sel of BANDS) {
    expect(native.band[sel], `${sel} is a second metric set: ` +
      `${native.band[sel]}px in a desktop window against ${web.band[sel]}px in the page`)
      .toBe(web.band[sel]);
  }
  // What the desktop window still gains is **regions**, not pixels off a band:
  // it draws no branding strip (the OS title bar carries the document) and no
  // menu bar (the OS draws one). Asserted as exactly those two, rather than as
  // a loose floor, because "the difference is the regions we dropped" is the
  // claim — a grid that gained more than that has taken it from somewhere
  // unaccounted for.
  const dropped = web.region.header + web.region.menubar;
  expect(native.grid - web.grid, `the sheet gained ${native.grid - web.grid}px where the strip ` +
    `(${web.region.header}px) and the menu bar (${web.region.menubar}px) are what desktop chrome drops`)
    .toBeCloseTo(dropped, 0);
  // And they are gone rather than merely shorter, which is the other way the
  // arithmetic above could come out right.
  expect(native.region, "a region desktop chrome does not draw is still taking height")
    .toEqual({ header: 0, menubar: 0 });

  // **No pointer target is under 28px, in either chrome.**
  //
  // This was `min(web, 28)` — a floor relative to the page's own control, which
  // was the right shape while the page had a second set of metrics to be
  // measured against. With one set it would assert nothing at all: web and
  // native are the same number, so any pair of equal values passes. It is a
  // flat 28 now, which is what its own prose always claimed — LibreOffice's
  // toolbar buttons run ~26-30, OnlyOffice's 32-36, Excel's ribbon larger
  // again, and ~28 is where a mouse target starts needing aim. It is also the
  // floor `docs/88` §3.2's "26 x 26" would have gone under; that table reads
  // "now (desktop) 26x26" against a tree that predates this correction.
  //
  // One exception, named rather than derived: the sheet tab is 26px and has
  // been in the page as well, so it is a metric this editor chose and not
  // something desktop density took away. Written down here because with one
  // metric set `web` can no longer disagree and so can no longer say it.
  const FLOOR = { ".sheet-tab": 26 };
  const measured = Object.keys(native.box);
  expect(measured.length, "nothing was measurable; the selectors have moved")
    .toBeGreaterThanOrEqual(NATIVE_CONTROLS.length - 1);
  for (const sel of measured) {
    const floor = FLOOR[sel] ?? 28;
    expect(native.box[sel].h, `${sel} is ${native.box[sel].h}px — under the ${floor}px it is held to`)
      .toBeGreaterThanOrEqual(floor);
  }

  // **Density is a box metric. It may not shrink type at all.**
  //
  // Worth an assertion because the reported complaint was "too small" and the
  // obvious suspect — the font — turned out never to have moved: every one of
  // these carried its web size already, bar the font-name and font-size fields,
  // which lost half a pixel to a `font-size: 12px` that bought nothing. Small
  // *text* is what "too small" usually means, so this is the half of the
  // invariant that is easiest to break by accident.
  for (const [sel, size] of Object.entries(native.type)) {
    expect(size, `${sel} renders ${size}px in a desktop window and ${web.type[sel]}px in the page`)
      .toBeGreaterThanOrEqual(web.type[sel]);
  }
});

/// **A mode can be turned off again.**
///
/// `setCapabilities({ mode })` is a host surface, so a relocation has to be
/// reversible; a one-way move would leave the page's own header permanently
/// missing its status line the moment anything switched modes.
test("leaving desktop chrome puts the moved node back", async ({ page }) => {
  await boot(page, "?chrome=native");
  expect(
    await page.evaluate(() => !!document.querySelector(".bottom-bar #presence")),
    "the roster was never relocated",
  ).toBe(true);

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ mode: "standalone" }));
  await expect(page.locator(".app-header")).toBeVisible();

  expect(
    await page.evaluate(() => !!document.querySelector("#menubar #presence")),
    "the roster went home",
  ).toBe(true);

  // **`#tb-status` is not in this list any more, and that is the fix rather
  // than a gap in it** (`UX-CHR-03`). It was relocated because it lived in the
  // branding strip and desktop chrome dropped that strip; the strip now carries
  // the *document*, and the status line is authored in `.bottom-bar` for every
  // chrome — so there is nothing to move and nothing to put back. Asserted here,
  // at the moment a mode is turned off, because "the node stayed where it
  // belongs" and "the relocation quietly stopped working" look identical from
  // the desktop side alone.
  const status = await seen(page, "#tb-status");
  expect(status.inBottomBar, "the status line moved out of the status bar").toBe(true);
  expect(status.painted, "and it cannot be read").toBe(true);
});


// --- The desktop menu speaks the platform's vocabulary (`TAURI-009`) ----------
//
// Reported from the desktop build: it offers **"Download ▸ Excel"** where every
// desktop application on every platform says *Save As*. Downloading is what a
// browser does — bytes land in a folder the user did not choose and the
// document has no home to go back to. A desktop app writes a file.
//
// The menu is derived from one `MENUS` literal, so the fix is one tree with two
// vocabularies rather than a second definition of the File menu. What makes
// that safe is that **the id does not move**: `commandId()` builds ids from the
// English label, the native shell dispatches `file.download.excel-xlsx` and
// nothing else, hosts name those ids in `setCommandRules`, and
// `CAPABILITY_COMMANDS` governs the export entries with `/^file\.download/`.
// Renaming the menu must not rename the command, or `canSaveAs` quietly stops
// applying to the entries it exists to gate — which is why one test below
// withholds the permission in the renamed mode and looks.

/// Every label in the model, flattened, so "no entry says Download" can be
/// asserted over the whole tree rather than over the entries we thought of.
const labels = (page) =>
  page.evaluate(() => {
    const out = [];
    const walk = (items) => {
      for (const i of items) {
        if (i.kind === "separator") continue;
        out.push(i.label);
        if (i.kind === "submenu") walk(i.items);
      }
    };
    for (const m of window.opencalcEditor.menuModel()) { out.push(m.label); walk(m.items); }
    return out;
  });

/// The File menu as label/id pairs, which is the shape both halves of this turn
/// on: the wording a user reads, and the id the shell dispatches.
const fileMenu = (page) =>
  page.evaluate(() => {
    const file = window.opencalcEditor.menuModel().find((m) => m.label === "File");
    const out = [];
    const walk = (items, depth) => {
      for (const i of items) {
        if (i.kind === "separator") continue;
        out.push({ depth, label: i.label, id: i.id });
        if (i.kind === "submenu") walk(i.items, depth + 1);
      }
    };
    walk(file.items, 0);
    return out;
  });

/// **No entry in a desktop window says Download.**
test("the desktop menu says Save and Export, never Download", async ({ page }) => {
  await boot(page, "?chrome=native");
  const file = await fileMenu(page);
  const top = file.filter((i) => i.depth === 0).map((i) => i.label);

  expect(top.slice(0, 4), `File read ${JSON.stringify(top)}`)
    .toEqual(["New", "Open…", "Save", "Export"]);
  // `Ctrl+S` has committed the document to its own file since `SAVE-02` and was
  // reachable by that chord alone. A desktop application whose File menu offers
  // no Save is one a user concludes cannot save.
  expect(file.find((i) => i.id === "file.save").label).toBe("Save");
  // And the one entry under Export that is not a conversion — it writes the
  // document back in the kind of file it came from — is named for that.
  expect(file.find((i) => i.id === "file.download.same-format-as-opened").label)
    .toBe("Save a copy…");

  const said = (await labels(page)).filter((l) => /download/i.test(l));
  expect(said, "a desktop window used the browser's word for writing a file").toEqual([]);
});

/// **The web build keeps saying Download, because that is what happens there.**
///
/// The inverse defect, and the one a careless fix introduces: in a browser tab
/// there is no file to commit to, so an entry labelled *Save* would be the
/// desktop vocabulary leaking the other way. `file.save` is therefore absent
/// from the web menu entirely — and absent means unrunnable, not merely
/// unlisted, which is the rule `runCommand` already keeps for a hidden id.
test("the web build still says Download, and has no Save to mislead with", async ({ page }) => {
  await boot(page);
  const file = await fileMenu(page);
  const top = file.filter((i) => i.depth === 0).map((i) => i.label);

  expect(top.slice(0, 3)).toEqual(["New", "Open…", "Download"]);
  expect(top, "a browser tab has no file to save to").not.toContain("Save");
  expect(file.find((i) => i.id === "file.download.same-format-as-opened").label)
    .toBe("Same format as opened");

  expect(await page.evaluate(() => window.opencalcEditor.listCommands()))
    .not.toContain("file.save");
  const refused = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("file.save"); return "it ran"; }
    catch (e) { return e.message; }
  });
  expect(refused, "hidden in the menu and runnable from a script is not hidden")
    .toMatch(/not available in this mode/);
});

/// **The wording moved; the ids did not.**
///
/// The whole hazard of relabelling a menu whose ids are derived from its
/// labels. Asserted twice over, because the second half is what catches a fix
/// that renamed the command as well: the ids are still `file.download.*`, *and*
/// `canSaveAs` — whose only pattern for these is `/^file\.download/` — still
/// takes every one of them away in a native window.
test("renaming Download to Export does not rename the commands it gates", async ({ page }) => {
  await boot(page, "?chrome=native");

  const file = await fileMenu(page);
  expect(file.find((i) => i.label === "Export").id).toBe("file.download");
  expect(file.filter((i) => i.depth === 1).map((i) => i.id)).toEqual([
    "file.download.same-format-as-opened",
    "file.download.excel-xlsx",
    "file.download.excel-macro-enabled-xlsm",
    "file.download.opendocument-ods",
    "file.download.csv-csv",
    "file.download.tab-separated-tsv",
    "file.download.pipe-separated-psv",
  ]);

  // A host withholding the permission, in the mode where the menu is renamed.
  await page.evaluate(() => window.opencalcEditor.setCapabilities({ canSaveAs: false }));
  const withheld = (await fileMenu(page)).map((i) => i.id);
  expect(withheld.filter((id) => /^file\.download/.test(id)),
    "Export survived a mode that forbids taking a copy out").toEqual([]);
  expect(withheld, "Save survived it too").not.toContain("file.save");
});

/// **Leaving desktop chrome puts the web wording back.**
///
/// `setCapabilities({ mode })` is a host surface and every other part of this
/// relocation is reversible; a one-way rename would leave a browser tab saying
/// "Export" the moment anything had switched modes.
test("leaving desktop chrome restores the browser's wording", async ({ page }) => {
  await boot(page, "?chrome=native");
  expect((await fileMenu(page)).find((i) => i.id === "file.download").label).toBe("Export");

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ mode: "standalone" }));
  const web = await fileMenu(page);
  expect(web.find((i) => i.id === "file.download").label).toBe("Download");
  expect(web.find((i) => i.id === "file.download.same-format-as-opened").label)
    .toBe("Same format as opened");
  expect(web.map((i) => i.id), "Save stayed in a window with no file").not.toContain("file.save");

  await page.evaluate(() => window.opencalcEditor.setCapabilities({ mode: "desktop" }));
  expect((await fileMenu(page)).find((i) => i.id === "file.download").label).toBe("Export");
});


// --- Theme is a display option, and the strip is not a toolbar (`UX-CHR-01`) --
//
// Two placement defects reported from a running editor, both with an answer
// already in the product.
//
// Theme was a `<select>` inside the settings gear popover: two clicks, in the
// title area, and a different mental model from the four display toggles that
// sit directly above it in the View menu — Gridlines, Cell markings, Formulas
// instead of results, Zero values. Excel and Sheets both put the appearance
// control in a View menu and neither hides it behind a gear.
//
// `#hdr-open` was a folder icon in the branding strip that clicked the same
// picker `File ▸ Open` does. A branding strip carries identity and document
// state; an action there duplicates a menu item and makes the region a toolbar.
// It is also the control `UX-DESK-05` found still *listed* by `listCommands()`
// in desktop chrome, where the strip is not drawn at all — the list naming a
// command with nothing to click.

/// Theme's own corner of the View menu, as the model reports it.
const themeSub = (page) =>
  page.evaluate(() => {
    const view = window.opencalcEditor.menuModel().find((m) => m.label === "View");
    const sub = view.items.find((i) => i.id === "view.theme");
    return {
      present: !!sub,
      items: sub ? sub.items.map((i) => ({ label: i.label, id: i.id, checked: i.checked })) : [],
      rendered: document.documentElement.dataset.theme ?? null,
    };
  });

/// **Theme is in View, it works from there, and the tick follows the choice.**
///
/// The last assertion is the one a half-fix fails: a menu that applies a theme
/// but reads its tick back from `document.documentElement.dataset.theme` cannot
/// tell Auto from Light — that attribute is absent for both — so it would mark
/// Light in a window the user had set to Auto.
test("Theme is a View menu option and its tick follows the choice", async ({ page }) => {
  await boot(page);

  const at = await themeSub(page);
  expect(at.present, "Theme is not in the View menu").toBe(true);
  expect(at.items.map((i) => i.label)).toEqual(["Auto", "Light", "Dark"]);
  expect(at.items.filter((i) => i.checked).map((i) => i.label), "a fresh editor is on Auto")
    .toEqual(["Auto"]);

  // Run it the way the operating system's own menu would.
  expect(await page.evaluate(() => window.opencalcEditor.runCommand("view.theme.dark"))).toBe(true);
  await expect
    .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
    .toBe("dark");
  const dark = await themeSub(page);
  expect(dark.items.filter((i) => i.checked).map((i) => i.label),
    "exactly one theme is ticked, and it is the chosen one").toEqual(["Dark"]);

  // Back to Auto, whose rendered state is indistinguishable from Light on a
  // light host: the tick has to come from the choice, not from the screen.
  await page.evaluate(() => window.opencalcEditor.runCommand("view.theme.auto"));
  const auto = await themeSub(page);
  expect(auto.rendered, "Auto stamps nothing, which is why it cannot be read back").toBe(null);
  expect(auto.items.filter((i) => i.checked).map((i) => i.label),
    "Auto was not ticked, or Light was ticked in its place").toEqual(["Auto"]);
});

/// **Moved, not copied.** Two controls for one setting drift apart, and this
/// pair would have: the `<select>` was the only thing that knew the chosen
/// value, so a theme picked from the menu would have left it showing the
/// previous one — a settings panel stating the opposite of the current theme.
test("the settings popover no longer carries a second theme control", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("tools.settings"));
  await expect(page.locator("#settings-panel")).toBeVisible();

  expect(await page.locator("#set-theme").count(), "two controls for one setting").toBe(0);
  // The panel still holds the settings that have no menu home, which is what
  // makes the assertion above mean "theme left" rather than "the panel broke".
  await expect(page.locator("#set-scroll")).toBeVisible();
  await expect(page.locator("#set-accent")).toBeVisible();
});

// --- A query string is a request, not a menu (`UX-CHR-02`) -------------------
//
// Reported by a user running the editor: *"it doesn't have header as well as
// file menus .. and no way on screen to change theme"*.
//
// `UX-DESK-01` hid the branding strip **and** the menu bar on `.oc-chrome-native`
// alone, which `?chrome=native` sets from the URL. Inside the Tauri shell that
// is right: the operating system draws a menu bar in its place. In an ordinary
// browser nothing does, so the same query string produced a window with no File
// menu, no View menu, and — the appearance control being a `<select>` inside the
// gear that lived in the strip — no way to reach the theme at all.
//
// **The rule is that hiding a menu requires evidence another menu exists.** The
// evidence is the shell's own bridge, not a request on a URL. These are the
// tests that could not have been written before `installShell()`, and their
// absence is exactly why this shipped: run in a browser, `?chrome=native`
// changes CSS and nothing can tell you whether a native bar took over.

/// **Asking for desktop chrome does not take the menus away.**
test("chrome=native in a browser keeps its menu bar, because nothing replaced it", async ({ page }) => {
  await boot(page, "?chrome=native");

  await expect(page.locator("#menubar"), "the menus went and nothing drew any")
    .toBeVisible();
  const onBar = await page.evaluate(() =>
    [...document.querySelectorAll(".menubar button.menu-top")]
      .filter((b) => !b.hidden && b.getBoundingClientRect().height > 0)
      .map((b) => b.textContent.trim()));
  expect(onBar, "File is not on the bar").toContain("File");
  expect(onBar, "View is not on the bar").toContain("View");

  // The density half of desktop chrome is still applied — this is not a revert
  // of `UX-DESK-01`, it is a split of what that row bundled together.
  await expect(page.locator(".app-header")).toBeHidden();
  expect(await page.evaluate(() => document.querySelector(".toolbar").getBoundingClientRect().height))
    .toBeLessThan(49);

  // **And the bar the fix put back may not carry a control that cannot act.**
  //
  // `#hdr-collapse` collapses the page header, and it lives in the menu bar —
  // so while the bar was hidden too, nobody could see that this chrome has no
  // header for it to collapse. Measured before it was hidden: the caret was on
  // screen offering "Hide the page header", the header was already 0px, and
  // clicking it moved the grid by 0px while relabelling itself "Show the page
  // header". That is the `header.open` defect again, uncovered by fixing this
  // one — which is the reason to assert it here rather than trust the CSS.
  await expect(page.locator("#hdr-collapse"), "a caret that collapses a header this chrome does not draw")
    .toBeHidden();
});

/// **The shell still gets its bar back, and the sheet still gets the height.**
///
/// The other half of the same claim: the fix must not be "never hide the menu
/// bar", which would undo `UX-DESK-01` rather than correct it.
test("the shell's bridge is the evidence that hides the bar", async ({ page }) => {
  await boot(page, "?chrome=native");
  const withoutShell = await page.evaluate(() => ({
    barSeen: document.getElementById("menubar").getBoundingClientRect().height > 0,
    grid: document.querySelector("#grid").getBoundingClientRect().height,
  }));

  await bootShell(page);
  const withShell = await page.evaluate(() => ({
    barSeen: document.getElementById("menubar").getBoundingClientRect().height > 0,
    grid: document.querySelector("#grid").getBoundingClientRect().height,
  }));

  expect(withoutShell.barSeen, "a browser keeps the bar").toBe(true);
  expect(withShell.barSeen, "the shell does not").toBe(false);
  expect(withShell.grid - withoutShell.grid, "and the bar's height goes to the sheet")
    .toBeGreaterThan(20);
});

/// **The theme is reachable on screen in every chrome.**
///
/// Location is not the promise; `UX-CHR-01` moved theme into the View menu, and
/// that only helps if a View menu is on screen — which in `?chrome=native` it
/// was not. So this asserts *reachability*: in each chrome, find a control a
/// pointer can actually get to, click through to it, and watch the theme change.
/// A rule that hides whatever holds it fails here rather than in a user report.
for (const [name, how] of [
  ["the page", (page) => boot(page)],
  ["a browser asked for desktop chrome", (page) => boot(page, "?chrome=native")],
  ["the desktop shell", (page) => bootShell(page)],
]) {
  test(`the theme can be changed on screen in ${name}`, async ({ page }) => {
    await how(page);

    const route = await page.evaluate(() => {
      // Visible means painted and inside the window, not merely attached: the
      // defect was a control that existed in a `display: none` ancestor.
      const shown = (el) => {
        if (!el) return false;
        const r = el.getBoundingClientRect();
        const s = getComputedStyle(el);
        return s.display !== "none" && s.visibility !== "hidden" && r.width > 0 && r.height > 0;
      };
      // The HTML menu bar, when the editor is drawing one...
      const view = [...document.querySelectorAll(".menubar button.menu-top")]
        .find((b) => b.dataset.ocLabel === "View");
      if (shown(view)) return "menubar";
      // ...or the operating system's.
      //
      // **`menuModel()` alone is not evidence of a route, and this is the trap
      // the first draft of this test fell into.** The model is derived from the
      // DOM and a CSS-hidden ancestor does not remove a node from it, so a
      // browser with the bar hidden reports a complete View menu that nothing
      // is drawing — and the test passed against the exact defect it was
      // written for. A native menu counts only when something is there to draw
      // it, which is the same evidence `applyModeChrome()` requires.
      const drawnNatively = !!(window.__opencalcNative || window.__TAURI__);
      const model = window.opencalcEditor.menuModel().find((m) => m.label === "View");
      const theme = model?.items.find((i) => i.id === "view.theme");
      if (drawnNatively && theme && theme.items.length === 3) return "native-menu";
      return "nowhere";
    });
    expect(route, "there is no route to the theme in this chrome").not.toBe("nowhere");

    // Reached, not merely listed. `runCommand` refuses an id the rules have
    // hidden, so this fails if the entry is present-but-unreachable.
    expect(await page.evaluate(() => window.opencalcEditor.runCommand("view.theme.dark"))).toBe(true);
    await expect
      .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
      .toBe("dark");
  });
}

/// **The branding strip carries identity and state, and no file action.**
///
/// And the id goes with the button: `listCommands()` is derived from the live
/// DOM, so deleting the control is what makes the list stop naming it. That is
/// `UX-DESK-05` settled in the direction that keeps "listed implies reachable
/// by pointer" true, rather than the one that keeps the id and hides it.
test("the branding strip holds no file action, in either chrome", async ({ page }) => {
  await boot(page);

  const inStrip = await page.evaluate(() =>
    [...document.querySelector(".app-header").querySelectorAll("[data-oc-command]")]
      .map((n) => n.dataset.ocCommand));
  expect(inStrip.filter((id) => /^file\.|^header\./.test(id)),
    `the strip is a toolbar again: ${JSON.stringify(inStrip)}`).toEqual([]);
  expect(await page.locator("#hdr-open").count(), "the folder icon is back in the strip").toBe(0);

  for (const query of ["", "?chrome=native"]) {
    if (query) await boot(page, query);
    const ids = await page.evaluate(() => window.opencalcEditor.listCommands());
    expect(ids, `header.open is listed in ${query || "the page"} with nothing to click`)
      .not.toContain("header.open");
    const refused = await page.evaluate(() => {
      try { window.opencalcEditor.runCommand("header.open"); return "it ran"; }
      catch (e) { return e.message; }
    });
    expect(refused).toMatch(/unknown OpenCalc command/);
    // Opening a file has not been taken away — only the second route to it.
    expect(ids).toContain("file.open");
  }
});
