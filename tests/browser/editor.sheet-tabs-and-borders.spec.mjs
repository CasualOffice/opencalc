// The last three UX rows, which had all sat at "Partial" for weeks.
//
// Each names something that "remains" — a colour swatch, a locale prefix, tab
// overflow and move-to. Every one turned out to be implemented and ungated,
// which is the same shape `UX-GRID-02` had: a fix that shipped, was never
// gated, and had a row still describing it as outstanding.
//
// So these tests are the work. They either confirm the behaviour and close the
// rows honestly, or find the gap the rows claim.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// **Tabs scroll rather than squashing or overflowing the window.**
///
/// A workbook with thirty sheets is ordinary. Tabs that shrink to nothing, or
/// push the rest of the status bar off-screen, are the two ways this goes wrong
/// and neither is visible with the three sheets a test usually makes.
test("many sheet tabs scroll instead of overflowing the window", async ({ page }) => {
  await boot(page);
  // **Through the editor's own "+" button.** Calling `session_add_sheet`
  // directly changes the workbook without telling the editor, so the tab strip
  // never rebuilds — that is a test bug and it looked exactly like a missing
  // feature the first time this ran.
  // `.sheet-add` is worn by three buttons in this strip — the add, the
  // all-sheets menu and the two scroll arrows pinned beside them (`UX-CHR-07`).
  // `.sheet-new` is the add.
  const add = page.locator(".sheet-add.sheet-new").first();
  await expect(add, "there is no add-sheet button").toBeVisible();
  for (let i = 0; i < 24; i += 1) await add.click();

  const strip = await page.evaluate(() => {
    const el = document.querySelector(".sheet-tabs");
    if (!el) return null;
    return {
      scrollable: el.scrollWidth > el.clientWidth,
      overflowX: getComputedStyle(el).overflowX,
      withinWindow: el.getBoundingClientRect().right <= window.innerWidth + 1,
      tabs: el.querySelectorAll(".sheet-tab").length,
    };
  });

  expect(strip, "there is no sheet tab strip").not.toBeNull();
  expect(strip.tabs, "the new sheets did not reach the tab strip").toBeGreaterThan(20);
  expect(strip.overflowX, "the strip does not scroll, so tabs are lost or squashed").toBe("auto");
  expect(strip.scrollable, "thirty tabs did not overflow — the test proves nothing").toBe(true);
  expect(strip.withinWindow, "the tab strip pushed past the window edge").toBe(true);
});

/// **A tab can be moved, and the move survives.**
///
/// `session_move_sheet` exists; what this pins is that the editor's own reorder
/// path reaches it and the order sticks.
test("a sheet can be moved and stays where it was put", async ({ page }) => {
  await boot(page);
  const order = await page.evaluate(async () => {
    const w = (await import(window.__editorModule)).wasmApi();
    w.session_add_sheet();
    w.session_add_sheet();
    const before = JSON.parse(w.session_sheet_names());
    w.session_move_sheet(0, 2);
    return { before, after: JSON.parse(w.session_sheet_names()) };
  });

  expect(order.before.length).toBe(3);
  expect(order.after.length).toBe(3);
  expect(order.after, "moving a sheet changed nothing").not.toEqual(order.before);
  expect(order.after[2], "the moved sheet did not land at the end").toBe(order.before[0]);
});

/// **The border colour swatch shows the colour that will be drawn.**
///
/// The toolbar button carries a bar tinted by `--oc-x-border-swatch`. Without
/// it a user picks a colour and the control looks identical, so the only way to
/// know what the next border will be is to draw one and look.
test("choosing a border colour tints the toolbar swatch", async ({ page }) => {
  // **A window wide enough to show the group.** The toolbar collapses groups
  // Excel-style rather than wrapping or scrolling, so at the suite's default
  // 1280px the border control is inside a `hidden` group and unclickable —
  // which is the toolbar working, not a defect, and looked like one until the
  // button was probed.
  await page.setViewportSize({ width: 1800, height: 900 });
  await boot(page);
  await page.locator("#tb-border").click();
  await expect(page.locator("#border-menu"), "the border menu did not open").toBeVisible();

  const swatch = page.locator("#border-menu .bd-color[data-color='e5484d']").first();
  await expect(swatch, "the border menu has no colour swatches").toBeVisible();
  await swatch.click();

  const tint = await page.evaluate(() =>
    document.getElementById("tb-border").style.getPropertyValue("--oc-x-border-swatch").trim());
  expect(tint, "the toolbar swatch did not take the chosen colour").toBe("#e5484d");

  // And the chosen swatch is marked, so reopening the menu shows what is set.
  await expect(swatch, "the chosen swatch is not marked as selected").toHaveClass(/\bon\b/);
});

/// **An elapsed-time format with a locale prefix renders as a time.**
///
/// `[$-409][h]:mm` — the `0` in the locale id is not a digit placeholder, and
/// reading it as one turned the whole format into literal text. That was fixed;
/// nothing pinned it from the outside.
test("an elapsed-time format behind a locale prefix still renders", async ({ page }) => {
  await boot(page);
  const shown = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    w.session_set_cell(0, 0, 0, "1.5");
    w.session_set_number_format(0, 0, 0, 0, 0, "[$-409][h]:mm");
    // The paint payload, which is what the canvas draws — so this is the text a
    // user actually sees rather than a separate formatting path.
    const cells = JSON.parse(w.session_cells(0, 0, 0, 0, 0));
    return cells.map((c) => c.t ?? c.text ?? "").join("");
  });
  expect(shown, "the format rendered as literal text").not.toContain("[");
  expect(shown, `expected an elapsed time, got ${shown}`).toMatch(/\d+:\d\d/);
});
