// The sheet-tab strip's navigation controls, and whether they can be reached
// (`UX-CHR-07`).
//
// Found by running the editor, not by reading it: `renderTabs()` appends the
// add-sheet `+` and the all-sheets `☰` **into `#sheet-tabs`**, which is the
// `overflow-x: auto` element. Once the tabs overflow, the two controls are
// pushed past the strip's right edge along with the tabs — and the strip has no
// scroll arrows at all, so nothing in the chrome brings them back. A user with
// twelve sheets on a 1280px window cannot add a thirteenth.
//
// So these assert *reachability*, not appearance: a control that is in the DOM,
// styled correctly, and clipped out of its scroller is not a control.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// The add button, wherever it currently lives.
///
/// `.sheet-add` is also worn by the all-sheets menu (`.sheet-all`) and by the
/// scroll arrows (`.sheet-scroll`), which share its 26px square shape.
const ADD = ".sheet-add:not(.sheet-all):not(.sheet-scroll)";

/// Add sheets through the editor's own `+`.
///
/// Not `session_add_sheet()`: that changes the workbook without telling the
/// editor, so the strip never rebuilds and the overflow this is about never
/// happens.
///
/// Playwright scrolls the button into view before each click, which is the one
/// thing a user has no control that offers — that is the defect, and it is why
/// the assertions below hit-test rather than click.
async function addSheets(page, n) {
  const add = page.locator(ADD).first();
  await expect(add, "there is no add-sheet button").toBeVisible();
  for (let i = 0; i < n; i += 1) await add.click();
}

/// Is this element actually reachable — on screen, and the thing under its own
/// centre point?
///
/// `elementFromPoint` is what makes this honest. A button scrolled out of an
/// `overflow: auto` ancestor still has a bounding box and still reports as
/// "visible" to a naive check; it is simply not painted there, and the hit test
/// is what notices.
function reachability() {
  return (selector) => {
    const el = document.querySelector(selector);
    if (!el) return { found: false };
    const r = el.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    const hit = document.elementFromPoint(cx, cy);
    return {
      found: true,
      onScreen:
        r.width > 0 && r.height > 0 &&
        r.left >= 0 && r.top >= 0 &&
        r.right <= window.innerWidth && r.bottom <= window.innerHeight,
      clickable: !!(hit && (hit === el || el.contains(hit))),
      insideScroller: !!el.closest(".sheet-tabs"),
      strip: (() => {
        const s = document.querySelector(".sheet-tabs");
        return { scrollWidth: s.scrollWidth, clientWidth: s.clientWidth, scrollLeft: s.scrollLeft };
      })(),
    };
  };
}

test("twelve sheets do not scroll the add-sheet and all-sheets buttons out of reach", async ({ page }) => {
  await boot(page);
  // Eleven clicks on top of the seeded sheet: twelve in all, the count docs/88
  // measured overflowing a 1280px window (897px of content in a 796px strip).
  await addSheets(page, 11);

  const add = await page.evaluate(reachability(), ADD);
  const all = await page.evaluate(reachability(), ".sheet-all");

  expect(add.found, "there is no add-sheet button at all").toBe(true);
  expect(all.found, "there is no all-sheets button at all").toBe(true);
  // The premise: without overflow this test proves nothing.
  expect(
    add.strip.scrollWidth,
    "twelve sheets did not overflow the strip — this window is too wide for the defect",
  ).toBeGreaterThan(add.strip.clientWidth);

  expect(add.clickable, "the add-sheet + is not the element under its own centre — it scrolled out of the tab strip").toBe(true);
  expect(all.clickable, "the all-sheets ☰ is not the element under its own centre — it scrolled out of the tab strip").toBe(true);
  expect(add.onScreen, "the add-sheet + is not on screen").toBe(true);
  expect(all.onScreen, "the all-sheets ☰ is not on screen").toBe(true);

  // And the reason it can never come back: both are pinned outside the
  // scrolling element, so no number of tabs can push them anywhere.
  expect(add.insideScroller, "the add-sheet + is still inside the scrolling strip").toBe(false);
  expect(all.insideScroller, "the all-sheets ☰ is still inside the scrolling strip").toBe(false);
});

test("a thirteenth sheet can be added with twelve already there", async ({ page }) => {
  await boot(page);
  await addSheets(page, 11);

  // A raw mouse click at the page coordinates the button reports, with no
  // `scrollIntoViewIfNeeded` in front of it. That is the whole difference: a
  // locator click scrolls the strip until the button is under the cursor, and a
  // user has no control that does. So this clicks where the button *says* it
  // is, and only lands on it if that is where it is painted.
  const where = await page.evaluate((sel) => {
    const r = document.querySelector(sel).getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  }, ADD);
  await page.mouse.click(where.x, where.y);

  await expect(
    page.locator(".sheet-tab"),
    "clicking where the + is drawn added no sheet — the click went somewhere else",
  ).toHaveCount(13);
});

test("the tab strip has scroll arrows once it overflows", async ({ page }) => {
  await boot(page);
  await addSheets(page, 11);

  const prev = page.locator(".sheet-scroll-prev");
  const next = page.locator(".sheet-scroll-next");
  await expect(prev, "there is no scroll-left arrow").toBeVisible();
  await expect(next, "there is no scroll-right arrow").toBeVisible();

  // Home, then walk right: the arrows must move the strip, and must stop
  // rather than run off the end.
  await page.evaluate(() => { document.querySelector(".sheet-tabs").scrollLeft = 0; });
  const at = () => page.evaluate(() => document.querySelector(".sheet-tabs").scrollLeft);
  await expect(prev, "the left arrow is offered at the left end").toBeDisabled();

  const before = await at();
  await next.click();
  const after = await at();
  expect(after, "the right arrow did not scroll the strip").toBeGreaterThan(before);

  for (let i = 0; i < 30 && !(await next.isDisabled()); i += 1) await next.click();
  const end = await page.evaluate(() => {
    const s = document.querySelector(".sheet-tabs");
    return { left: s.scrollLeft, max: s.scrollWidth - s.clientWidth };
  });
  expect(end.left, "the right arrow did not reach the end of the strip").toBeGreaterThanOrEqual(end.max - 1);
  await expect(next, "the right arrow is still offered at the right end").toBeDisabled();

  await prev.click();
  expect(await at(), "the left arrow did not scroll back").toBeLessThan(end.left);
});

/// **Jumping to a sheet whose tab is off to the left leaves it off screen.**
///
/// A second defect, found while measuring the first. `renderTabs()` scrolls the
/// active tab into view with `tabsEl.scrollLeft = activeTab.offsetLeft`, and
/// `offsetLeft` is only that tab's position *inside the strip* if the strip is
/// the tab's `offsetParent`. `.sheet-tabs` is statically positioned, so it is
/// not one, and every offset is inflated by the strip's own distance from the
/// left of the page — the strip over-scrolls by exactly that much and leaves
/// the tab it was aiming at clipped off the left edge.
///
/// It is small enough to be invisible while the strip starts ~14px from the
/// page edge, and it stops being invisible the moment anything is pinned to the
/// strip's left.
test("jumping to the first sheet from the far end brings its tab into view", async ({ page }) => {
  await boot(page);
  await addSheets(page, 11);
  await page.evaluate(() => {
    const s = document.querySelector(".sheet-tabs");
    s.scrollLeft = s.scrollWidth;
  });

  // Through the all-sheets menu, which is the route a user has to a tab that is
  // not on screen — and the whole point of the control this row pins down.
  await page.locator(".sheet-all").click();
  await page.locator("#sheet-ctx .menu-item", { hasText: /^Sheet1$/ }).click();

  const seen = await page.evaluate(() => {
    const s = document.querySelector(".sheet-tabs");
    const t = s.querySelector(".sheet-tab.active");
    const sr = s.getBoundingClientRect();
    const tr = t.getBoundingClientRect();
    return { name: t.textContent, left: tr.left - sr.left, right: sr.right - tr.right };
  });
  expect(seen.name, "the all-sheets menu did not switch sheet").toBe("Sheet1");
  expect(seen.left, "Sheet1's tab is clipped off the left of the strip").toBeGreaterThanOrEqual(-1);
  expect(seen.right, "Sheet1's tab is clipped off the right of the strip").toBeGreaterThanOrEqual(-1);
});
