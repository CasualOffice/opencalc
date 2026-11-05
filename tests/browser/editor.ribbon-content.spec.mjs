// Ribbon content parity (XL-UX-037, 040, 041, 042, 043).
//
// Excel's groups were a thin subset: Conditional Formatting dropped two items,
// the Formulas tab had no Function Library, Page Layout three big buttons, and
// Review a "New Note" that opened a comment thread. Each test here drives a
// control the way a user would — the ribbon button, then the menu or dialog —
// and reads the result back from the workbook, because a control that opens
// and writes nothing looks exactly like one that works.
//
// Only controls whose feature exists are drawn (docs/95 rule 9): Icon Sets,
// A Date Occurring, Show Comments, Spelling and the rest are absent, and the
// first test says so in its expectation rather than leaving it implicit.

import { expect, test } from "@playwright/test";

async function boot(page, cells = []) {
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate((cells) => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (const [r, c, v] of cells) a.session_set_cell(0, r, c, v);
    window.opencalcEditor.selectForTest(0, 0);
  }, cells);
  await page.locator("#grid").focus();
}

const api = (page, fn, ...args) => page.evaluate(([fn, args]) => window.opencalcEditor.wasmApi()[fn](...args), [fn, args]);
const json = async (page, fn, ...args) => JSON.parse(await api(page, fn, ...args));
const selectRange = (page, r0, c0, r1, c1) => page.evaluate(([r0, c0, r1, c1]) => {
  window.opencalcEditor.selectForTest(r0, c0);
  window.opencalcEditor.extendSelectionForTest(r1, c1);
}, [r0, c0, r1, c1]);
const tab = (page, name) => page.locator('.oc-ribbon [role="tab"]', { hasText: new RegExp(`^${name}$`) }).click();
const ribbonButton = (page, id) => page.locator(`.oc-ribbon .rb-panel:not([hidden]) [data-oc-proxy="${id}"]`).first();
const pop = (page) => page.locator(".rb-pop").first();
const item = (page, id) => page.locator(`.rb-pop .rb-pop-item[data-cmd="${id}"]`);
const sub = (page, label) => page.locator(`.rb-pop .rb-pop-subitem[data-sub="${label}"]`);
/// The visible labels of one menu level, separators as "-".
const labels = (list) => list.evaluate((el) => [...el.children]
  .filter((c) => c.matches(".rb-pop-item, .rb-pop-sep"))
  .map((c) => (c.matches(".rb-pop-sep") ? "-" : c.firstChild.textContent.trim())));
const modalBtn = (page, name) => page.locator("#oc-modal-body").getByRole("button", { name, exact: true });

test("Conditional Formatting drops Excel's menu, in Excel's order", async ({ page }) => {
  await boot(page);
  await tab(page, "Home");
  await ribbonButton(page, "format.conditional-formatting").click();
  // Icon Sets › is Excel's fifth item; it is absent until icon sets draw.
  expect(await labels(pop(page))).toEqual([
    "Highlight Cells Rules", "Top/Bottom Rules", "Data Bars", "Color Scales", "-",
    "New Rule…", "Clear Rules", "Manage Rules…",
  ]);
  await sub(page, "Highlight Cells Rules").hover();
  const highlight = page.locator('.rb-pop-sub[aria-label="Highlight Cells Rules"]');
  await expect(highlight).toBeVisible();
  // A Date Occurring… is left out: the engine has no time-period rule.
  expect(await labels(highlight)).toEqual([
    "Greater Than…", "Less Than…", "Between…", "Equal To…", "Text that Contains…", "Duplicate Values…", "-", "More Rules…",
  ]);
  await sub(page, "Top/Bottom Rules").hover();
  await expect(highlight, "hovering a sibling closes the first submenu").toHaveCount(0);
  expect(await labels(page.locator('.rb-pop-sub[aria-label="Top/Bottom Rules"]'))).toEqual([
    "Top 10 Items…", "Top 10%…", "Bottom 10 Items…", "Bottom 10%…", "Above Average…", "Below Average…", "-", "More Rules…",
  ]);
});

test("Greater Than… paints Excel's default preset: light red fill with dark red text", async ({ page }) => {
  await boot(page, [[0, 0, "150"], [1, 0, "20"], [2, 0, "90"]]);
  await selectRange(page, 0, 0, 2, 0);
  await tab(page, "Home");
  await ribbonButton(page, "format.conditional-formatting").click();
  await sub(page, "Highlight Cells Rules").hover();
  await item(page, "format.cf.greater-than").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Greater Than");
  await expect(page.locator(".oc-cfq-lead")).toHaveText("Format cells that are GREATER THAN:");
  await expect(page.locator("#oc-cfq-preset option:checked")).toHaveText("Light Red Fill with Dark Red Text");
  await page.locator("#oc-cfq-a").fill("100");
  await modalBtn(page, "OK").click();
  await expect(page.locator("#oc-modal")).toBeHidden();
  const rules = await json(page, "session_cf_rules", 0);
  expect(rules.map((r) => [r.range, r.desc, r.fill, r.font])).toEqual([["A1:A3", "greater than 100", "FFC7CE", "9C0006"]]);
  const cells = await json(page, "session_cells", 0, 0, 0, 2, 0);
  const a1 = cells.find((c) => c.r === 0);
  expect([a1.bg, a1.fc], "the matching cell takes both halves of the preset").toEqual(["FFC7CE", "9C0006"]);
  expect(cells.find((c) => c.r === 1).bg ?? "", "a cell below the threshold is left alone").toBe("");
});

test("a quick rule refuses an operand it cannot use, in a message box over the field", async ({ page }) => {
  await boot(page, [[0, 0, "1"]]);
  await page.evaluate(() => window.opencalcEditor.runCommand("format.cf.top-10-items"));
  await expect(page.locator("#oc-cfq-a"), "Top 10 Items starts at Excel's 10").toHaveValue("10");
  await page.locator("#oc-cfq-a").fill("0");
  await modalBtn(page, "OK").click();
  await expect(page.locator("#oc-msgbox-text")).toHaveText("Enter an integer between 1 and 1000.");
  expect(await json(page, "session_cf_rules", 0), "nothing was written").toEqual([]);
});

test("Equal To… with a reference is the same test, written over the range", async ({ page }) => {
  await boot(page, [[0, 0, "5"], [1, 0, "6"], [0, 2, "6"]]);
  await selectRange(page, 0, 0, 1, 0);
  await page.evaluate(() => window.opencalcEditor.runCommand("format.cf.equal-to"));
  await page.locator("#oc-cfq-a").fill("=$C$1");
  await page.locator("#oc-cfq-preset").selectOption({ label: "Green Fill with Dark Green Text" });
  await modalBtn(page, "OK").click();
  const cells = await json(page, "session_cells", 0, 0, 0, 1, 0);
  expect(cells.find((c) => c.r === 1)?.bg, "A2 equals C1").toBe("C6EFCE");
  expect(cells.find((c) => c.r === 0)?.bg ?? "", "A1 does not").toBe("");
});

test("Data Bars, Color Scales and Clear Rules act on the selection and the sheet", async ({ page }) => {
  await boot(page, [[0, 0, "1"], [1, 0, "2"], [0, 1, "3"], [1, 1, "4"]]);
  await selectRange(page, 0, 0, 1, 0);
  await tab(page, "Home");
  await ribbonButton(page, "format.conditional-formatting").click();
  await sub(page, "Data Bars").hover();
  await item(page, "format.cf.data-bar.green").click();
  await selectRange(page, 0, 1, 1, 1);
  await ribbonButton(page, "format.conditional-formatting").click();
  await sub(page, "Color Scales").hover();
  await item(page, "format.cf.color-scale.green-yellow-red").click();
  expect((await json(page, "session_cf_rules", 0)).map((r) => [r.range, r.desc])).toEqual([
    ["A1:A2", "data bar"], ["B1:B2", "colour scale (3 stops)"],
  ]);
  await ribbonButton(page, "format.conditional-formatting").click();
  await sub(page, "Clear Rules").hover();
  await item(page, "format.cf.clear-sheet").click();
  expect(await json(page, "session_cf_rules", 0), "Clear Rules from Entire Sheet").toEqual([]);
});

test("the Formulas tab has Excel's Function Library, and a function opens Function Arguments", async ({ page }) => {
  await boot(page);
  await tab(page, "Formulas");
  const group = page.locator('.rb-panel:not([hidden]) .rb-group[aria-label="Function Library"]');
  const buttons = await group.locator(".rb-items > .rb-btn").evaluateAll((bs) => bs.filter((b) => !b.hidden).map((b) => b.getAttribute("aria-label")));
  expect(buttons).toEqual(["Insert Function", "AutoSum", "Recently Used", "Financial", "Logical", "Text",
    "Date & Time", "Lookup & Reference", "Math & Trig", "More Functions"]);
  await ribbonButton(page, "formulas.library.financial").click();
  const pmt = page.locator('.rb-pop .rb-pop-item[data-entry="PMT"]');
  await expect(pmt).toBeVisible();
  await expect(pmt, "the tooltip is Excel's syntax line").toHaveAttribute("title", /^PMT\(rate,nper,pv,fv,type\)/);
  await expect(page.locator('.rb-pop .rb-pop-item[data-entry="SUM"]'), "Financial lists only financial functions").toHaveCount(0);
  await pmt.click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Function Arguments");
  await expect(page.locator(".oc-fa-fn")).toHaveText("PMT");
  await modalBtn(page, "Cancel").click();
  // Recently Used now leads with the function just chosen.
  await ribbonButton(page, "formulas.library.recently-used").click();
  await expect(page.locator(".rb-pop .rb-pop-item[data-entry]").first()).toHaveText("PMT");
  await page.keyboard.press("Escape");
  // More Functions › Statistical is a cascade, not a flat list.
  await ribbonButton(page, "formulas.more-functions").click();
  expect(await labels(pop(page))).toEqual(["Statistical", "Engineering", "Information", "Compatibility", "Web"]);
  await sub(page, "Statistical").hover();
  await expect(page.locator('.rb-pop-sub[aria-label="Statistical"] .rb-pop-item[data-entry="STDEV.S"]')).toBeVisible();
});

test("AutoSum › Average writes AVERAGE over the run above", async ({ page }) => {
  await boot(page, [[0, 0, "2"], [1, 0, "4"]]);
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 0));
  await tab(page, "Formulas");
  await ribbonButton(page, "formulas.autosum").click();
  expect(await labels(pop(page))).toEqual(["Sum", "Average", "Count Numbers", "Max", "Min", "-", "More Functions…"]);
  await item(page, "formulas.autosum.average").click();
  expect(await api(page, "session_cell_input", 0, 2, 0)).toBe("=AVERAGE(A1:A2)");
});

test("Page Layout sets orientation, margins, fit and print options in the page setup", async ({ page }) => {
  await boot(page);
  await tab(page, "Page Layout");
  const caps = await page.locator(".rb-panel:not([hidden]) .rb-cap-text").allTextContents();
  expect(caps).toEqual(["Page Setup", "Scale to Fit", "Sheet Options"]);
  await ribbonButton(page, "page.orientation").click();
  await item(page, "page.orientation.landscape").click();
  await ribbonButton(page, "page.margins").click();
  await item(page, "page.margins.narrow").click();
  await page.locator('.oc-ribbon [data-oc-proxy="page.fit.width"]').click();
  await page.locator(".rb-combo-list .rb-pop-item", { hasText: /^1 page$/ }).click();
  await page.locator('.oc-ribbon .rb-check[data-cmd="sheet.gridlines.print"]').click();
  const p = await json(page, "session_page_setup", 0);
  expect({
    orientation: p["page.orientation"], left: p["margins.left"], top: p["margins.top"],
    fit: p["setupPr.fitToPage"], width: p["page.fitToWidth"], grid: p["options.gridLines"],
  }).toEqual({ orientation: "landscape", left: "0.25", top: "0.75", fit: "1", width: "1", grid: "1" });
  await expect(page.locator('.oc-ribbon .rb-check[data-cmd="sheet.gridlines.print"]')).toHaveAttribute("aria-checked", "true");
  // Excel greys Scale while Width or Height fits the page.
  await expect(page.locator('.oc-ribbon input[data-oc-proxy="page.fit.scale"]')).toBeDisabled();
  // Margins › ticks the preset in force.
  await ribbonButton(page, "page.margins").click();
  await expect(item(page, "page.margins.narrow")).toHaveAttribute("aria-checked", "true");
});

test("Page Layout ▸ Breaks inserts and removes a break at the active cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 2));
  await tab(page, "Page Layout");
  await ribbonButton(page, "page.breaks").click();
  await item(page, "page.breaks.insert").click();
  expect(await json(page, "session_page_breaks", 0)).toEqual({ rows: [4], cols: [2] });
  await ribbonButton(page, "page.breaks").click();
  await item(page, "page.breaks.insert").click();
  expect(await json(page, "session_page_breaks", 0), "Insert again does not take them away").toEqual({ rows: [4], cols: [2] });
  await ribbonButton(page, "page.breaks").click();
  await item(page, "page.breaks.reset").click();
  expect(await json(page, "session_page_breaks", 0)).toEqual({ rows: [], cols: [] });
});

test("Review ▸ Comments: Next, Previous and Delete walk and remove the threads", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_comment(0, 1, 1, "first", "", "");
    a.session_set_comment(0, 5, 0, "second", "", "");
  });
  await tab(page, "Review");
  const caps = await page.locator(".rb-panel:not([hidden]) .rb-cap-text").allTextContents();
  expect(caps, "Comments, not a Notes group that opened a comment").toEqual(["Comments", "Protect"]);
  await ribbonButton(page, "review.next-comment").click();
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toMatchObject({ r0: 1, c0: 1 });
  await ribbonButton(page, "review.next-comment").click();
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toMatchObject({ r0: 5, c0: 0 });
  await ribbonButton(page, "review.previous-comment").click();
  expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toMatchObject({ r0: 1, c0: 1 });
  await ribbonButton(page, "review.delete-comment").click();
  expect((await json(page, "session_comments", 0, 0, 0, 100, 100)).map((c) => [c.r, c.c])).toEqual([[5, 0]]);
});

test("View ▸ Show, Zoom and Freeze Panes are Excel's controls", async ({ page }) => {
  await boot(page);
  await tab(page, "View");
  const fb = page.locator('.oc-ribbon .rb-check[data-cmd="view.show.formula-bar"]');
  await expect(fb).toHaveAttribute("aria-checked", "true");
  await fb.click();
  await expect(page.locator(".formula-bar"), "Formula Bar unticked hides it").toBeHidden();
  await fb.click();
  await expect(page.locator(".formula-bar")).toBeVisible();
  // Zoom… is Excel's dialog.
  await ribbonButton(page, "view.zoom.dialog").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Zoom");
  await page.locator("#oc-zoom-75").check();
  await modalBtn(page, "OK").click();
  expect(await page.evaluate(() => window.opencalcEditor.state.zoom)).toBe(0.75);
  // Zoom to Selection: a small block fills the window, so the zoom goes up.
  await selectRange(page, 0, 0, 3, 2);
  await ribbonButton(page, "view.zoom.selection").click();
  expect(await page.evaluate(() => window.opencalcEditor.state.zoom)).toBeGreaterThan(1);
  // Freeze Panes ›: the first item becomes Unfreeze Panes once frozen.
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 1));
  await ribbonButton(page, "view.freeze").click();
  expect(await labels(pop(page))).toEqual(["Freeze Panes", "Freeze Top Row", "Freeze First Column"]);
  await item(page, "view.freeze.panes").click();
  await ribbonButton(page, "view.freeze").click();
  await expect(item(page, "view.freeze.panes")).toHaveText("Unfreeze Panes");
});

test("a read-only workbook keeps View's own controls and loses the ones that write", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setReadOnly(true));
  await tab(page, "View");
  // Whether the formula bar shows is the page's chrome, not the workbook's;
  // a viewer keeps it, as a viewer keeps the zoom.
  await expect(page.locator('.oc-ribbon .rb-check[data-cmd="view.show.formula-bar"]')).toBeVisible();
  await expect(ribbonButton(page, "view.zoom.selection")).toBeVisible();
  // Gridlines is a sheet setting saved in the file, which read-only refuses.
  await expect(page.locator('.oc-ribbon .rb-panel:not([hidden]) .rb-check[data-cmd="sheet.gridlines.view"]')).toBeHidden();
  await tab(page, "Page Layout");
  await expect(ribbonButton(page, "page.orientation"), "a page-setup write").toBeHidden();
  await tab(page, "Formulas");
  await expect(ribbonButton(page, "formulas.library.financial"), "a function writes into a cell").toBeHidden();
});
