// What a finger can hit, and what it costs the sheet.
//
// Measured on a phone (390×844) and a tablet (820×1180) with touch, before any
// of this: **not one** of the editor's 27 pointer targets reached 44px, the iOS
// minimum. Eight were under 24px, which is the floor `docs/68` already enforces
// elsewhere in this product:
//
//     fx-insert 16x16 | fx-expand 17x17 | name-box-list 18x22
//     zoom-out 20x20  | zoom-in 20x20   | zoom-level 42x20
//     hdr-collapse 26x22 | tb-brand 65x17 | zoom-slider 84x4 (tablet)
//
// Two things are asserted together on purpose, because either alone is a trap:
//
//   - **Bigger.** Menu rows, header buttons, toolbar buttons and the formula
//     bar's three smallest controls.
//   - **And at a bounded, accounted-for cost to the sheet.** UX-EDIT-01 settled
//     that every pixel of chrome comes out of the grid, and this asserted the
//     strict form of it — `#grid` exactly as tall with a finger as with a mouse
//     — which held while the web toolbar was 48px and had room for a 44px
//     target inside it. `UX-CHR-05` collapsed the web and desktop metrics into
//     one measured set (a 42px toolbar of 28px buttons), and a 44px target does
//     not fit in a 42px band. The two bands that hold one now grow back under
//     `(pointer: coarse)` and a phone gives up 12px of sheet; what is asserted
//     is that the loss is *exactly* that band growth and no more. See the
//     assertion itself for why not growing them would have been worse than the
//     12px: `.toolbar` is `overflow-y: hidden`, so the floor would have read as
//     kept while the control was clipped.
//     The page must also not widen. That was violated by the first attempt at
//     this: growing the status bar's zoom controls pushed the document to
//     439px on a 390px screen, and Chrome rescaled the whole layout viewport to
//     fit — 844px of window reporting itself as 951. `editor.narrow-screens`
//     already forbids that, but only with a mouse, so a rule that fires solely
//     under `(pointer: coarse)` was invisible to it. That is why the width
//     assertion is repeated here with touch on.

import { expect, test } from "@playwright/test";

const TOUCH = [
  { name: "phone", width: 390, height: 844 },
  { name: "tablet", width: 820, height: 1180 },
];

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(400);
}

/** Every row of every menu panel that is open right now. */
const openMenuRows = (page) => page.evaluate(() => {
  const rows = [];
  for (const panel of document.querySelectorAll(".menu-drop, .menu-sub, .ctx-menu, .tb-more-flyout")) {
    if (panel.hidden || panel.getBoundingClientRect().height === 0) continue;
    for (const b of panel.querySelectorAll("button")) {
      const r = b.getBoundingClientRect();
      if (r.height === 0) continue;
      rows.push({ panel: panel.className.split(" ")[0], t: b.textContent.trim().slice(0, 22), h: Math.round(r.height) });
    }
  }
  return rows;
});

for (const size of TOUCH) {
  test(`menu rows are 44px on a ${size.name}, and the sheet pays only the bands that hold one`, async ({ browser }) => {
    // Two contexts at the same size — one with a finger, one with a mouse. The
    // difference between them *is* the change, which is the only way to show
    // that nothing here leaked onto the desktop.
    const coarseCtx = await browser.newContext({ viewport: size, hasTouch: true, isMobile: true });
    const fineCtx = await browser.newContext({ viewport: size });
    const coarse = await coarseCtx.newPage();
    const fine = await fineCtx.newPage();
    await boot(coarse);
    await boot(fine);

    expect(await coarse.evaluate(() => matchMedia("(pointer: coarse)").matches), "the finger context is not coarse, so this proves nothing").toBe(true);
    expect(await fine.evaluate(() => matchMedia("(pointer: coarse)").matches)).toBe(false);

    // Format is the longest menu and holds every dangerous neighbour pair.
    const openFormat = (page) => page.evaluate(async () => {
      [...document.querySelectorAll(".menubar .menu-top")].find((b) => b.textContent.trim() === "Format").click();
      await new Promise((r) => setTimeout(r, 250));
    });
    await openFormat(coarse);
    await openFormat(fine);

    const coarseRows = await openMenuRows(coarse);
    const fineRows = await openMenuRows(fine);
    expect(coarseRows.length, "no menu opened").toBeGreaterThan(8);
    expect(
      coarseRows.filter((r) => r.h < 44),
      "menu rows under 44px with a finger",
    ).toEqual([]);
    // Scoped, not global: the same markup with a mouse keeps the dense rows.
    expect(Math.min(...fineRows.map((r) => r.h)), "the dense desktop rows changed too").toBeLessThan(40);

    // A taller menu than the screen has to scroll, not spill: `anchorMenu`
    // clamps it, and clipping would look identical from the rect alone.
    const drop = await coarse.evaluate(() => {
      const d = [...document.querySelectorAll(".menu-drop")].find((e) => !e.hidden && e.getBoundingClientRect().height > 0);
      const r = d.getBoundingClientRect();
      return { bottom: +r.bottom.toFixed(1), vh: window.innerHeight, scrolls: d.scrollHeight > d.clientHeight + 1, overflowY: getComputedStyle(d).overflowY };
    });
    expect(drop.bottom, `the Format menu ends at ${drop.bottom} of ${drop.vh}`).toBeLessThanOrEqual(drop.vh + 0.5);
    if (drop.scrolls) expect(drop.overflowY).toBe("auto");

    await coarse.keyboard.press("Escape");
    await fine.keyboard.press("Escape");
    await coarse.waitForTimeout(150);

    // **The sheet pays the two bands that hold a 44px control, and nothing
    // else** — and this used to say "the sheet pays nothing".
    //
    // It could, while the web toolbar was 48px and its buttons 30: a finger's
    // 44px fitted in the room the band already had. `UX-CHR-05` collapsed the
    // web and desktop metric sets into one and the surviving set is the
    // measured, desktop one — a 42px toolbar of 28px buttons and a 36px formula
    // bar — which has no such room. A 44px target does not fit in a 42px band,
    // and `.toolbar` is `overflow-y: hidden`, so *not* growing the band would
    // have clipped the target to 41px while `getBoundingClientRect` went on
    // reporting 44: the floor would read as kept and would not be. So the two
    // bands that hold one grow back to their touch heights under
    // `(pointer: coarse)` and a phone gives up 12px of sheet for it.
    //
    // The invariant that remains is the one that was actually at stake: the
    // growth is **bounded and accounted for**. Every pixel the coarse chrome
    // costs the grid is a band that had to hold a floor-sized control, computed
    // here rather than written down, so a rule that quietly takes height for
    // anything else still fails. Two chromes' worth of density and a 44px
    // finger cannot both be free, and this is where the trade is recorded.
    const chromeOf = (page) => page.evaluate(() => {
      const h = (sel) => document.querySelector(sel).getBoundingClientRect().height;
      return { grid: Math.round(h("#grid")), toolbar: h(".toolbar"), formulaBar: h(".formula-bar") };
    });
    const coarseChrome = await chromeOf(coarse);
    const fineChrome = await chromeOf(fine);
    const bandGrowth = Math.round(
      (coarseChrome.toolbar - fineChrome.toolbar) + (coarseChrome.formulaBar - fineChrome.formulaBar),
    );
    expect(coarseChrome.grid, `the grid lost ${fineChrome.grid - coarseChrome.grid}px to the chrome ` +
      `where the bands grew ${bandGrowth}px`).toBe(fineChrome.grid - bandGrowth);
    // And the bands may only grow by what a floor-sized control needs. 16px is
    // the two bands at 48 and 40, which is where they were before `UX-CHR-05`
    // tightened them and is the most a touch pointer may ever cost the sheet.
    expect(bandGrowth, "a coarse pointer is taking more height than its floors need")
      .toBeLessThanOrEqual(16);

    // And the page does not widen — the failure the first attempt at this hit,
    // and the one `editor.narrow-screens` cannot see because it uses a mouse.
    const width = await coarse.evaluate(() => ({
      doc: document.documentElement.scrollWidth, inner: window.innerWidth,
      widest: (() => {
        let w = 0, who = null;
        for (const e of document.querySelectorAll("*")) {
          const r = e.getBoundingClientRect();
          if (r.width > 0 && r.right > w) { w = r.right; who = `${e.tagName.toLowerCase()}#${e.id}.${String(e.className).split(" ")[0]}`; }
        }
        return { right: Math.round(w), who };
      })(),
    }));
    expect(width.doc, `document is ${width.doc}px in a ${width.inner}px window; widest is ${JSON.stringify(width.widest)}`)
      .toBeLessThanOrEqual(width.inner);
    expect(await coarse.evaluate(() => window.innerHeight), "the layout viewport was rescaled, which only happens when the page overflows").toBe(size.height);

    await coarseCtx.close();
    await fineCtx.close();
  });
}

test("the controls that were under 24px are not, on a phone", async ({ browser }) => {
  // A touch context, not `setViewportSize` on the default one: none of this is
  // keyed on width. `setViewportSize` alone leaves the pointer fine and every
  // assertion below reads the desktop sizes — which is how this test first
  // failed, and is exactly the scoping it is meant to check.
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  const page = await ctx.newPage();
  await boot(page);
  const measured = await page.evaluate(() => {
    const out = {};
    // `hdr-open` used to be measured here too. The branding strip's folder
    // button is gone (`UX-CHR-01`) — a file action does not belong in the one
    // region that carries identity and document state — so there is nothing
    // left to size. `tb-settings` below is the other 44px control in that
    // strip and keeps the claim about it honest.
    for (const id of ["fx-insert", "fx-expand", "name-box-list", "tb-settings", "tb-undo", "hdr-collapse"]) {
      const r = document.querySelector("#" + id).getBoundingClientRect();
      out[id] = { w: Math.round(r.width), h: Math.round(r.height) };
    }
    return out;
  });

  // The formula bar's three smallest controls: 16×16, 17×17 and 18×22 before.
  // 40px rather than 44 for the first two, and 26×34 for the caret, because the
  // bar is 40px tall and none of this is allowed to make it taller.
  for (const id of ["fx-insert", "fx-expand"]) {
    expect(measured[id], `${id} ${JSON.stringify(measured[id])}`).toMatchObject({ w: 40, h: 40 });
  }
  expect(measured["name-box-list"].w, "the name-box caret was 18px wide").toBeGreaterThanOrEqual(24);
  expect(measured["name-box-list"].h, "and 22px tall").toBeGreaterThanOrEqual(24);

  // Header and toolbar have the room, so they get the full 44.
  expect(measured["tb-settings"]).toMatchObject({ w: 44, h: 44 });
  expect(measured["tb-undo"]).toMatchObject({ w: 44, h: 44 });
  // The menu bar is 30px tall and stays that way, so this one only clears 24.
  expect(Math.min(measured["hdr-collapse"].w, measured["hdr-collapse"].h)).toBeGreaterThanOrEqual(24);
  await ctx.close();
});

test("the long-press context menu is reachable by finger, every row of it", async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  const page = await ctx.newPage();
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 40; r += 1) a.session_set_cell(0, r, 0, `row${r}`);
  });

  // The only route to cut/copy/paste/insert/delete on a touch device: there is
  // no right button. It was 41 rows of 33px.
  const cdp = await page.context().newCDPSession(page);
  const box = await page.locator("#grid").boundingBox();
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x + 200, y: box.y + 150 }] });
  await page.waitForTimeout(700);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await expect(page.locator("#sheet-ctx")).toBeVisible();

  const rows = await openMenuRows(page);
  expect(rows.length, "the context menu opened empty").toBeGreaterThan(8);
  expect(rows.filter((r) => r.h < 44), "context-menu rows under 44px").toEqual([]);

  const fits = await page.evaluate(() => {
    const m = document.querySelector("#sheet-ctx");
    const r = m.getBoundingClientRect();
    return { top: +r.top.toFixed(1), bottom: +r.bottom.toFixed(1), left: +r.left.toFixed(1), right: +r.right.toFixed(1), vw: window.innerWidth, vh: window.innerHeight, scrolls: m.scrollHeight > m.clientHeight + 1, overflowY: getComputedStyle(m).overflowY };
  });
  expect(fits.right, JSON.stringify(fits)).toBeLessThanOrEqual(fits.vw + 0.5);
  expect(fits.bottom, JSON.stringify(fits)).toBeLessThanOrEqual(fits.vh + 0.5);
  expect(fits.top).toBeGreaterThanOrEqual(-0.5);
  if (fits.scrolls) expect(fits.overflowY).toBe("auto");
  await ctx.close();
});
