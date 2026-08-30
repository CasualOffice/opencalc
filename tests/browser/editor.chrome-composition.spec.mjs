// What the editor's chrome is composed of — `docs/88`, rows `UX-CHR-03/04/05`.
//
// The complaint these answer is one sentence — *"nothing like a regular
// spreadsheet editing desktop tool"* — and `docs/88` measured it into three
// structural differences from Excel, LibreOffice Calc, OnlyOffice, Google
// Sheets and Numbers. Each is a *placement*, which is why they are asserted
// here rather than in Rust: geometry and computed style are the only evidence
// that a region is where the design says it is.
//
//   1. A 53px strip naming the **product**. None of the five names its product
//      in the editor; all five name the **document**, in a title bar. Ours
//      carried a logo, "OpenCalc", an `Alpha` pill and `engine v0.0.0`.
//   2. A 595x33px frosted panel of six statistics floating **over the cells it
//      describes**, while the status bar 49px below it sat empty in the middle.
//      All four grid competitors put this in the bottom strip; none floats it.
//   3. Grouping stated **twice and weakly** — a filled 10px capsule (a 3%
//      luminance step) *and* a 1px rule — where all four desktop competitors
//      use the rule alone; and two metric sets, web and desktop, that drift.
//
// Every assertion below reads a box or a computed value. "The class is applied"
// is not the promise here any more than it was in `editor.native-chrome`.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// A desktop shell as far as the page can tell — the same bridge
/// `editor.native-chrome` installs, and for the same reason: `?chrome=native`
/// alone is a *request*, and only the bridge is evidence that an operating
/// system is drawing the menu bar (`UX-CHR-02`).
const installShell = (page) =>
  page.addInitScript(() => {
    const noop = () => Promise.resolve(null);
    window.__opencalcNative = {
      open: noop, save: noop, saveTarget: noop, clearSaveTarget: noop,
      setDocument: noop, syncCapabilities: noop, publishMenu: noop,
    };
  });

async function bootShell(page, query = "?chrome=native") {
  await installShell(page);
  await boot(page, query);
}

/** Open a document under a name, the way the file picker does. */
const openAs = (page, name, text) =>
  page.evaluate(
    ([name, text]) => window.opencalcEditor.openBytes(new TextEncoder().encode(text), name),
    [name, text],
  );

// --- `UX-CHR-03` — the strip names the document ------------------------------

/// **The region survives on the web and carries the document, not the product.**
///
/// It is not deleted uniformly, and that is the decision `docs/88` §10.1 left
/// open: a desktop window has an OS title bar carrying document identity, so
/// the strip goes entirely there; a browser tab has none, so deleting it
/// uniformly would leave a web user unable to see which file they have open.
/// What goes in both chromes is the *product* — the logo, the wordmark, the
/// `Alpha` pill and the engine version.
test("the web strip carries the document's name and save state, not the product's", async ({ page }) => {
  await boot(page);

  const strip = page.locator(".app-header");
  await expect(strip, "a browser tab has no title bar, so the region stays").toBeVisible();

  const product = await page.evaluate(() => {
    const el = document.querySelector(".app-header");
    return {
      text: el.textContent.replace(/\s+/g, " ").trim(),
      marks: el.querySelectorAll(".brand-logo, .tb-brand, .badge").length,
    };
  });
  expect(product.marks, `the product mark is still in the strip: ${product.text}`).toBe(0);
  expect(product.text, "the strip still names the product").not.toMatch(/OpenCalc|Alpha|engine v/);

  // And it names the document instead — the name it was opened under, and
  // whether the work in it has been written out.
  await openAs(page, "figures.csv", "a,b\n1,2\n");
  await expect(strip, "the open file's name is not in the strip").toContainText("figures.csv");
  await expect(strip, "a freshly opened document is not dirty").toContainText(/saved/i);

  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "changed"));
  await expect(strip, "the save state does not follow the document").toContainText(/unsaved/i);

  // **And nothing in it acts on the document**, which is what stops the region
  // growing back into a toolbar — the failure `UX-CHR-01` already caught once,
  // when a folder icon here was a second route to File ▸ Open. An allowlist
  // rather than a pattern over verbs: `toolbar.settings` is the gear, which
  // opens the application's own preferences and touches no document, and
  // anything else appearing here has to be argued for by editing this line.
  const commands = await page.evaluate(() =>
    [...document.querySelector(".app-header").querySelectorAll("[data-oc-command]")]
      .map((n) => n.dataset.ocCommand).sort());
  expect(commands, "the strip has grown a command").toEqual(["toolbar.settings"]);
});

/// **`#tb-status` is engine status, not product identity.**
///
/// It is the engine version, the open/save progress line and every error the
/// editor reports. `UX-DESK-01` already moved it into the status bar for the
/// desktop, where Excel, LibreOffice and OnlyOffice all keep document state.
/// Nothing about that argument was desktop-only: it was in the branding strip
/// on the web because the branding strip was there, and it outlived it.
for (const [name, how] of [
  ["the page", (page) => boot(page)],
  ["a browser asked for desktop chrome", (page) => boot(page, "?chrome=native")],
  ["the desktop shell", (page) => bootShell(page)],
]) {
  test(`the engine status line is in the status bar in ${name}`, async ({ page }) => {
    await how(page);
    const where = await page.evaluate(() => {
      const el = document.getElementById("tb-status");
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return {
        inBottomBar: !!el.closest(".bottom-bar"),
        inHeader: !!el.closest(".app-header"),
        painted: s.display !== "none" && s.visibility !== "hidden" && r.width > 0 && r.height > 0,
      };
    });
    expect(where.inHeader, "engine status is still in the identity strip").toBe(false);
    expect(where.inBottomBar, "engine status is not in the status bar").toBe(true);
    expect(where.painted, "and it cannot be read").toBe(true);
  });
}

/// **Version and build are `Help ▸ About`'s**, which is where five of five put
/// them — not a badge in the chrome of every session.
test("the version left the strip for Help ▸ About, and is the engine's own", async ({ page }) => {
  await boot(page);

  const engine = await page.evaluate(() => window.opencalcEditor.wasmApi().version());
  const chrome = await page.evaluate(() =>
    document.querySelector(".app-header").textContent.replace(/\s+/g, " ").trim());
  expect(chrome, "a version badge is still in the chrome").not.toMatch(/v\d+\.\d+\.\d+/);

  await page.evaluate(() => window.opencalcEditor.runCommand("help.about-opencalc"));
  const about = await page.locator("#oc-modal-body").textContent();
  // Read from the build rather than written twice: a literal here and a
  // literal in the dialog is two versions, and the one that goes stale is the
  // one nobody is looking at.
  expect(about, `About does not carry the engine version (${engine})`).toContain(engine);
});

// --- `UX-CHR-04` — the summary is in the bar it belongs to -------------------

/// **A summary that covers the cells it describes.**
///
/// 595 x 33 px at `bottom: 16px; right: 24px`, `backdrop-filter: blur(14px)`,
/// an 11px radius and a popover shadow — over the grid, with the status bar
/// 49px below it empty in the middle. Excel, LibreOffice, OnlyOffice and Sheets
/// all put this in the bottom strip and none of them floats it.
test("the selection summary is text in the status bar, not a panel over the cells", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 3; r += 1) a.session_set_cell(0, r, 0, String(r + 1));
  });
  await page.fill("#cell-ref", "A1:A3");
  await page.press("#cell-ref", "Enter");
  await expect(page.locator("#sel-stats")).not.toHaveText("");

  const seen = await page.evaluate(() => {
    const el = document.querySelector("#sel-stats");
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const grid = document.querySelector("#grid").getBoundingClientRect();
    const bar = document.querySelector(".bottom-bar").getBoundingClientRect();
    return {
      text: el.textContent.replace(/\s+/g, " ").trim(),
      inStatusBar: !!el.closest(".bottom-bar"),
      position: s.position,
      blur: s.backdropFilter || s.webkitBackdropFilter,
      shadow: s.boxShadow,
      insideBar: r.top >= bar.top - 0.5 && r.bottom <= bar.bottom + 0.5,
      overGrid: !(r.right <= grid.left || r.left >= grid.right || r.bottom <= grid.top || r.top >= grid.bottom),
    };
  });

  expect(seen.inStatusBar, `the summary is not in the status bar: ${JSON.stringify(seen)}`).toBe(true);
  expect(seen.overGrid, "it still covers the cells it describes").toBe(false);
  expect(seen.insideBar, "it is not inside the bar it was moved to").toBe(true);
  expect(seen.position, "it is still positioned out of the flow").toBe("static");
  expect(seen.blur, "the frosted treatment is still on it").toMatch(/^(none|)$/);
  expect(seen.shadow, "the popover shadow is still on it").toBe("none");

  // Excel's default three, in Excel's order and Excel's wording. Six values a
  // user did not ask for is more than any competitor's default but OnlyOffice's
  // five, and Min/Max/Numbers stay available rather than shown.
  expect(seen.text).toBe("Average: 2 Count: 3 Sum: 6");
  expect(seen.text, "the six-statistic set is still being drawn").not.toMatch(/Min|Max|Numbers/);
});

// --- `UX-CHR-05` — grouping said once, and one metric set -------------------

/// **A group is a 1px rule and nothing else.**
///
/// A filled 10px-radius capsule *and* a rule is the same statement made twice
/// and weakly: the capsule is `--oc-surface-color` on a white bar, a 3%
/// luminance step, below the threshold at which a boundary reads at all.
/// LibreOffice declares `<toolbar:toolbarseparator/>` as a first-class element,
/// OnlyOffice renders `separator.short` as a `border-left`, Excel uses a thin
/// rule and Sheets a hairline divider. Material Design 3's own toolbar guidance
/// says to avoid rounded corners on the container, because it implies the
/// container expands on interaction — which is exactly how a row of filled
/// capsules reads.
test("a toolbar group is stated by a rule, not by a capsule", async ({ page }) => {
  // Wide enough that nothing has collapsed into a flyout, or half of these
  // measure 0x0 because they are not on the bar at all.
  await page.setViewportSize({ width: 1600, height: 900 });
  await boot(page);

  const bar = await page.evaluate(() => {
    const group = document.querySelector(".toolbar .tb-group:not(.controls)");
    const sep = document.querySelector(".toolbar .tb-sep");
    const btn = document.querySelector(".toolbar .tb-btn");
    const gs = getComputedStyle(group);
    const ss = getComputedStyle(sep);
    const bs = getComputedStyle(btn);
    const br = btn.getBoundingClientRect();
    const ts = getComputedStyle(document.querySelector(".toolbar"));
    return {
      groupBackground: gs.backgroundColor,
      groupRadius: parseFloat(gs.borderTopLeftRadius),
      groupPadding: parseFloat(gs.paddingLeft),
      sepWidth: +sep.getBoundingClientRect().width.toFixed(2),
      sepMargin: parseFloat(ss.marginLeft),
      button: { w: Math.round(br.width), h: Math.round(br.height), radius: parseFloat(bs.borderTopLeftRadius) },
      barGap: parseFloat(ts.columnGap),
    };
  });

  expect(bar.groupBackground, "the capsule is still filled").toMatch(/rgba\(0, 0, 0, 0\)|transparent/);
  expect(bar.groupRadius, "the capsule still has a radius").toBe(0);
  expect(bar.groupPadding, "the capsule still has padding of its own").toBe(0);
  expect(bar.sepWidth, "the rule that is now the only statement of grouping").toBeCloseTo(1, 1);
  // `6px rule 6px` is the budget: the rule's own margin is folded into the
  // bar's gap rather than added to it (`docs/88` §3.1).
  expect(bar.sepMargin, "the separator carries a margin on top of the bar's gap").toBe(0);
  expect(bar.barGap, "the bar's gap is still the web one").toBe(6);
  expect(bar.button.radius, "a toolbar button is still a pill").toBe(3);
  expect(bar.button, "the button metric").toMatchObject({ w: 28, h: 28 });
});

/// **One metric set, so the two chromes cannot drift.**
///
/// Two existed only because the web one was too loose; once it is right there
/// is nothing for the desktop one to correct, and `docs/88` §7's "one
/// composition, three mounts" is what makes the desktop and the page one editor
/// rather than two designs. Asserted per band and per control, in both chromes,
/// because a single number would be satisfied by a coincidence.
test("the desktop and the page draw one toolbar, at one set of metrics", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });

  const metrics = () =>
    page.evaluate(() => {
      const out = { band: {}, box: {} };
      for (const sel of [".toolbar", ".formula-bar", ".bottom-bar"]) {
        out.band[sel] = +document.querySelector(sel).getBoundingClientRect().height.toFixed(1);
      }
      for (const sel of [".toolbar .tb-btn", ".toolbar input.tb-font", "#formula-input", "#cell-ref", ".sheet-tab"]) {
        const r = document.querySelector(sel)?.getBoundingClientRect();
        if (r && r.height > 0) out.box[sel] = +r.height.toFixed(1);
      }
      return out;
    });

  await boot(page);
  const web = await metrics();
  await bootShell(page);
  const native = await metrics();

  expect(native.band, "the desktop bars are a second metric set").toEqual(web.band);
  expect(native.box, "the desktop controls are a second metric set").toEqual(web.box);
  // Not a vacuous pass: the selectors have to have found something.
  expect(Object.keys(native.box).length, "nothing was measurable; the selectors moved")
    .toBeGreaterThanOrEqual(4);
});
