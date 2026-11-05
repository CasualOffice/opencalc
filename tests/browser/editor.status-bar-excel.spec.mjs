// The bottom of the window, laid out and behaving the way Excel's is.
//
// Excel draws two rows under the grid, not one:
//
//   1. the **sheet-tab row** — navigation arrows, the tabs, then the horizontal
//      scrollbar sharing that row behind a draggable splitter (`XL-UX-033`,
//      `XL-UX-052`);
//   2. the **status bar** — the cell mode at the left, the selection summary,
//      and zoom at the right — which right-clicks to "Customize Status Bar"
//      (`XL-UX-030`) and shows the mode plus brief prompts, never an echo of
//      every command (`XL-UX-032`).
//
// OpenCalc put the tabs, "Ready", the summary and zoom in a single 34px row,
// had no horizontal scrollbar on a desktop-sized window at all (it lived over
// the grid and hid itself whenever the sheet's data fitted), answered a right-
// click on the bar with nothing, and left "ok" in the bar after every commit.

import { expect, test } from "@playwright/test";

async function boot(page, url = "/editor.html") {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(url);
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

const rect = (page, sel) =>
  page.evaluate((s) => {
    const el = document.querySelector(s);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
  }, sel);

// --- XL-UX-033 / XL-UX-052 — two rows, and the scrollbar in the tab row ------

test("the sheet tabs sit in their own row above the status bar", async ({ page }) => {
  await boot(page);
  const where = await page.evaluate(() => ({
    tabsInStatus: !!document.querySelector("#sheet-tabs").closest(".bottom-bar"),
    modeInStatus: !!document.querySelector("#cell-mode").closest(".bottom-bar"),
    zoomInStatus: !!document.querySelector("#zoom-widget").closest(".bottom-bar"),
  }));
  expect(where.tabsInStatus, "the sheet tabs are still inside the status bar").toBe(false);
  expect(where.modeInStatus, "the cell mode left the status bar").toBe(true);
  expect(where.zoomInStatus, "zoom left the status bar").toBe(true);

  const tabs = await rect(page, "#sheet-tabs");
  const status = await rect(page, ".bottom-bar");
  const grid = await rect(page, "#grid");
  // Under the grid, over the status bar: Excel's order from the top down.
  expect(tabs.top, "the tab row is not below the grid").toBeGreaterThanOrEqual(grid.bottom - 0.5);
  expect(tabs.bottom, "the tab row is not above the status bar").toBeLessThanOrEqual(status.top + 0.5);
});

test("the horizontal scrollbar shares the tab row and is there on a desktop window", async ({ page }) => {
  await boot(page);
  const bar = await rect(page, "#hscroll");
  const tabs = await rect(page, "#sheet-tabs");
  const shown = await page.evaluate(() => getComputedStyle(document.getElementById("hscroll")).display);
  expect(shown, "no horizontal scrollbar on a 1440px window").not.toBe("none");
  expect(bar.width, "the scrollbar has no width").toBeGreaterThan(100);
  // Same row as the tabs, to their right — not floating over the grid.
  expect(bar.top, "the scrollbar is not in the tab row").toBeGreaterThanOrEqual(tabs.top - 4);
  expect(bar.bottom, "the scrollbar is not in the tab row").toBeLessThanOrEqual(tabs.bottom + 4);
  expect(bar.left, "the scrollbar is not right of the tabs").toBeGreaterThanOrEqual(tabs.right - 0.5);
  const grid = await rect(page, "#grid");
  expect(bar.top, "the scrollbar still covers the grid").toBeGreaterThanOrEqual(grid.bottom - 0.5);
});

test("the scrollbar in the tab row scrolls the sheet", async ({ page }) => {
  await boot(page);
  // Data well past the window's right edge, so there is somewhere to go.
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 0, 80, "far");
    window.opencalcEditor.draw();
  });
  const box = await rect(page, "#hscroll");
  const before = await page.evaluate(() => window.opencalcEditor.state.scrollX);
  // Clicking the track right of the thumb pages right, as any scrollbar does.
  await page.mouse.click(box.right - 6, box.top + box.height / 2);
  const after = await page.evaluate(() => window.opencalcEditor.state.scrollX);
  expect(after, "clicking the track did not scroll").toBeGreaterThan(before);
});

test("dragging the splitter gives the tabs more of the row and the scrollbar less", async ({ page }) => {
  await boot(page);
  const split = await rect(page, "#sheet-bar-split");
  expect(split, "there is no splitter between the tabs and the scrollbar").not.toBeNull();
  const tabs0 = await rect(page, "#sheet-tabs");
  const bar0 = await rect(page, "#hscroll");
  const y = split.top + split.height / 2;
  await page.mouse.move(split.left + split.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(split.left + 150, y, { steps: 5 });
  await page.mouse.up();
  const tabs1 = await rect(page, "#sheet-tabs");
  const bar1 = await rect(page, "#hscroll");
  expect(tabs1.width - tabs0.width, "the tabs did not widen").toBeGreaterThan(100);
  expect(bar0.width - bar1.width, "the scrollbar did not narrow").toBeGreaterThan(100);
  // The thumb is laid out against the track it now has, not the old one.
  const thumb = await rect(page, "#hthumb");
  expect(thumb.right, "the thumb runs past its track").toBeLessThanOrEqual(bar1.right + 0.5);
});

// --- XL-UX-032 — the bar is not a debug log -----------------------------------

test("committing a cell does not write 'ok' into the status bar", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.type("42");
  await page.keyboard.press("Enter");
  await expect(page.locator("#tb-status")).not.toHaveText(/^ok$/);
  await expect(page.locator("#tb-status")).toHaveText("");
});

test("resizing a column does not report 'resized'", async ({ page }) => {
  await boot(page);
  const edge = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(1) + ed.colWAt(1), y: ed.HH / 2 };
  });
  const box = await rect(page, "#grid");
  await page.mouse.move(box.left + edge.x, box.top + edge.y);
  await page.mouse.down();
  await page.mouse.move(box.left + edge.x + 60, box.top + edge.y, { steps: 4 });
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => window.opencalcEditor.wasmApi().session_can_undo())).toBe(true);
  await expect(page.locator("#tb-status")).not.toHaveText(/resized/);
});

test("a message ends when the selection moves on", async ({ page }) => {
  await boot(page);
  // Any command's message stands in for all of them: Sort with nothing to
  // sort says so, and the next selection change must take it down.
  await page.evaluate(() => window.opencalcEditor.selectForTest(3, 3));
  await page.evaluate(() => { document.getElementById("tb-status").textContent = "nothing to sort"; window.opencalcEditor.draw(); });
  await expect(page.locator("#tb-status")).toHaveText("nothing to sort");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#tb-status")).toHaveText("");
});

test("a refused system clipboard is said once, not for the rest of the session", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const deny = () => Promise.reject(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true, value: { write: deny, writeText: deny, read: deny, readText: deny },
    });
    window.opencalcEditor.wasmApi().session_set_cell(0, 2, 0, "x");
    window.opencalcEditor.selectForTest(2, 0);
  });
  await page.evaluate(() => window.opencalcEditor.doCopy());
  await expect(page.locator("#tb-status")).toContainText(/clipboard/);
  await page.evaluate(() => window.opencalcEditor.selectForTest(6, 4));
  await expect(page.locator("#tb-status")).toHaveText("");
});

test("a collaboration state is not a message, and survives a selection change", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.holdStatus("reconnecting…");
    window.opencalcEditor.draw();
  });
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#tb-status")).toHaveText("reconnecting…");
});

// --- XL-UX-030 — Customize Status Bar -------------------------------------------

async function openCustomize(page) {
  const bar = await rect(page, ".bottom-bar");
  await page.mouse.click(bar.left + bar.width / 2, bar.top + bar.height / 2, { button: "right" });
  const menu = page.locator("#status-customize");
  await expect(menu).toBeVisible();
  return menu;
}

const item = (menu, name) =>
  menu.locator("[role=menuitemcheckbox]").filter({ has: menu.page().getByText(name, { exact: true }) });

test("right-clicking the status bar opens Customize Status Bar, in Excel's order", async ({ page }) => {
  await boot(page);
  const menu = await openCustomize(page);
  await expect(menu).toContainText("Customize Status Bar");
  const labels = await menu.locator("[role=menuitemcheckbox] .sb-item-label").allTextContents();
  expect(labels).toEqual([
    "Cell Mode", "Caps Lock", "Num Lock",
    "Average", "Count", "Numerical Count", "Minimum", "Maximum", "Sum",
    "Zoom Slider", "Zoom",
  ]);
  // Excel's defaults: on, except the lock keys and the three extra statistics.
  const checked = await menu.locator("[role=menuitemcheckbox]").evaluateAll((els) =>
    els.map((e) => e.getAttribute("aria-checked")));
  expect(checked).toEqual([
    "true", "false", "false",
    "true", "true", "false", "false", "false", "true",
    "true", "true",
  ]);
  // Each row reads out the value it would show, as Excel's does.
  // The mode the bar is showing, whatever it is ("Calculate" on a fresh blank
  // workbook today — a separate defect, not this menu's).
  const mode = await page.locator("#cell-mode").textContent();
  await expect(item(menu, "Cell Mode").locator(".sb-item-value")).toHaveText(mode);
  await expect(item(menu, "Zoom")).toContainText("100%");
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("ticking Minimum and Maximum adds them to the summary, and the choice is kept", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 3; r += 1) a.session_set_cell(0, r, 0, String((r + 1) * 2));
    window.opencalcEditor.selectForTest(0, 0); window.opencalcEditor.extendSelectionForTest(2, 0);
  });
  await expect(page.locator("#sel-stats")).toHaveText(/Average: 4\s+Count: 3\s+Sum: 12/);
  const menu = await openCustomize(page);
  await item(menu, "Minimum").click();
  // The menu stays open, as Excel's does, so several can be ticked in a row.
  await expect(menu).toBeVisible();
  await item(menu, "Maximum").click();
  await item(menu, "Numerical Count").click();
  await expect(item(menu, "Minimum")).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
  await expect(page.locator("#sel-stats")).toHaveText(
    /Average: 4\s+Count: 3\s+Numerical Count: 3\s+Min: 2\s+Max: 6\s+Sum: 12/);

  // A preference, not a session setting.
  await page.reload();
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    for (let r = 0; r < 3; r += 1) a.session_set_cell(0, r, 0, String((r + 1) * 2));
    window.opencalcEditor.selectForTest(0, 0); window.opencalcEditor.extendSelectionForTest(2, 0);
  });
  await expect(page.locator("#sel-stats")).toContainText("Min: 2");
});

test("unticking Cell Mode, Zoom Slider and Zoom takes them off the bar", async ({ page }) => {
  await boot(page);
  const menu = await openCustomize(page);
  await item(menu, "Cell Mode").click();
  await item(menu, "Zoom Slider").click();
  await item(menu, "Zoom").click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#cell-mode")).toBeHidden();
  await expect(page.locator("#zoom-slider")).toBeHidden();
  await expect(page.locator("#zoom-in")).toBeHidden();
  await expect(page.locator("#zoom-level")).toBeHidden();
});

test("Caps Lock, once ticked, shows while the key is on", async ({ page }) => {
  await boot(page);
  const menu = await openCustomize(page);
  await item(menu, "Caps Lock").click();
  await page.keyboard.press("Escape");
  const caps = page.locator("#sb-caps-lock");
  await expect(caps).toBeHidden();
  // The browser reports the lock state on every key event; this is one with it on.
  await page.evaluate(() => {
    const e = new KeyboardEvent("keydown", { key: "a", bubbles: true, modifierCapsLock: true });
    document.body.dispatchEvent(e);
  });
  await expect(caps).toBeVisible();
  await expect(caps).toHaveText("Caps Lock");
  await page.evaluate(() => {
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
  });
  await expect(caps).toBeHidden();
});
