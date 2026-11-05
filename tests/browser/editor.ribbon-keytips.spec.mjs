// Ribbon KeyTips, every level, as in Excel (XL-UX-009).
//
// Excel's KeyTips are a path, not a single hop: Alt badges the tabs (and the
// Quick Access Toolbar with digits), a tab's letter switches to it and badges
// every control on it, and the control's letters run it — Alt H B opens
// Borders, Alt H F C opens Font Color. A KeyTip of two letters waits for the
// second, and Escape steps back one level. The ribbon stopped after the first
// hop: a tab's letter opened the tab and the trail went cold, so no command
// on the ribbon could be reached by an Excel user's hands.
//
// The letters are Excel's own for every control Excel has; they are what
// those hands already know, and a different letter is worse than none.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await expect(page.locator(".oc-ribbon .rb-panel:not([hidden])")).toBeVisible();
  await page.locator("#grid").focus();
}

const alt = async (page) => { await page.keyboard.down("Alt"); await page.keyboard.up("Alt"); };
/// The KeyTips on screen now, as `{ key: command-or-tab }`.
const shown = (page) => page.evaluate(() => Object.fromEntries(
  [...document.querySelectorAll(".oc-ribbon .rb-tip")]
    .filter((t) => !t.hidden && t.getClientRects().length > 0)
    .map((t) => [t.textContent, t.dataset.target])));

test("Alt then a tab's letter badges every control on that tab with Excel's KeyTips", async ({ page }) => {
  await boot(page);
  await alt(page);
  await page.keyboard.press("h");
  await expect(page.locator("#rb-tab-home")).toHaveAttribute("aria-selected", "true");
  // Excel's documented Home KeyTips.
  await expect.poll(() => shown(page)).toMatchObject({
    V: "edit.paste", X: "edit.cut", C: "edit.copy", FP: "toolbar.painter",
    FF: "toolbar.font", FS: "toolbar.size", 1: "toolbar.bold", 2: "toolbar.italic", 3: "toolbar.underline",
    B: "toolbar.border", H: "toolbar.fillcolor", FC: "toolbar.fontcolor",
    AT: "format.alignment.top", AC: "format.alignment.center", W: "format.text-overflow.wrap", M: "toolbar.merge",
    N: "toolbar.numfmt", P: "toolbar.percent", K: "toolbar.comma",
    L: "format.conditional-formatting", T: "insert.table", I: "cells.insert", D: "cells.delete",
    U: "formulas.autosum", S: "data.sort-range", FD: "edit.find-replace",
  });
  // The tab letters are gone: this is the next level, not the same one.
  const keys = await shown(page);
  expect(Object.values(keys)).not.toContain("tab:insert");
});

test("Alt H B opens Borders", async ({ page }) => {
  await boot(page);
  await alt(page);
  await page.keyboard.press("h");
  await page.keyboard.press("b");
  await expect(page.locator("#border-menu")).toBeVisible();
  await expect(page.locator(".oc-ribbon .rb-tip"), "the KeyTips stayed up after the command ran").toHaveCount(0);
});

test("a two-letter KeyTip waits for its second letter: Alt H F C opens Font Color", async ({ page }) => {
  await boot(page);
  await alt(page);
  await page.keyboard.press("h");
  await page.keyboard.press("f");
  // After F only the F… KeyTips remain, and nothing has run.
  const after = await shown(page);
  expect(Object.keys(after).length, "F left no KeyTips up").toBeGreaterThan(0);
  expect(Object.keys(after).every((k) => k.startsWith("F")), `non-F KeyTips still shown: ${Object.keys(after)}`).toBe(true);
  await expect(page.locator("#fontcolor-menu")).toBeHidden();
  await page.keyboard.press("c");
  await expect(page.locator("#fontcolor-menu")).toBeVisible();
});

test("Escape steps back one level at a time", async ({ page }) => {
  await boot(page);
  await alt(page);
  await page.keyboard.press("h");
  await page.keyboard.press("f");
  await page.keyboard.press("Escape");
  await expect.poll(() => shown(page), "Escape after F should restore the whole Home level").toMatchObject({ 1: "toolbar.bold", FC: "toolbar.fontcolor" });
  await page.keyboard.press("Escape");
  await expect.poll(() => shown(page), "Escape from Home should go back to the tab letters").toMatchObject({ H: "tab:home", N: "tab:insert", F: "file" });
  await page.keyboard.press("Escape");
  await expect(page.locator(".oc-ribbon .rb-tip")).toHaveCount(0);
});

test("Quick Access Toolbar buttons get digits at the first level, and a digit runs one", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator("#grid").focus();
  await page.keyboard.type("42");
  await page.keyboard.press("Enter");
  const a1 = () => page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0));
  await expect.poll(a1).toBe("42");

  await alt(page);
  await expect.poll(() => shown(page)).toMatchObject({ 1: "file.save", 2: "edit.undo", 3: "edit.redo" });
  await page.keyboard.press("2");
  await expect.poll(a1, "Alt 2 did not undo").not.toBe("42");
});

test("a KeyTip letter never reaches the grid", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.locator("#grid").focus();
  const before = await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0));
  const boldBtn = page.locator('.oc-ribbon .rb-panel:not([hidden]) [data-cmd="toolbar.bold"]');
  const wasBold = await boldBtn.getAttribute("aria-pressed");
  await alt(page);
  await page.keyboard.press("h");
  await page.keyboard.press("1");   // Bold
  await expect(boldBtn, "Alt H 1 did not toggle Bold").toHaveAttribute("aria-pressed", wasBold === "true" ? "false" : "true");
  // Neither "h" nor "1" started a cell edit.
  await expect(page.locator("#inline-edit")).toBeHidden();
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 0, 0))).toBe(before);
});

test("Alt held with a tab's letter goes straight to that tab's KeyTips", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+n");
  await expect(page.locator("#rb-tab-insert")).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => shown(page)).toMatchObject({ V: "insert.pivottable", T: "insert.table" });
});

test("Alt with a letter inside a dialog is the dialog's, not the ribbon's", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+1");   // Format Cells
  await expect(page.locator("#oc-modal-title")).toHaveText("Format Cells");
  await page.keyboard.press("Alt+n");
  await expect(page.locator("#rb-tab-insert"), "Alt+N behind a dialog switched the ribbon").toHaveAttribute("aria-selected", "false");
  await expect(page.locator(".oc-ribbon .rb-tip")).toHaveCount(0);
  // Nor does a lone Alt raise the ribbon's letters over it: they would take
  // every key the dialog's fields are typed with.
  await alt(page);
  await expect(page.locator(".oc-ribbon .rb-tip"), "a lone Alt behind a dialog raised the ribbon's KeyTips").toHaveCount(0);
});

test("KeyTips on every tab are unique and none is the start of another", async ({ page }) => {
  // A KeyTip that is a prefix of another ("F" beside "FC") could never be
  // pressed: the first letter would wait for a second that never comes. Read
  // from the model the page serves, so every tab is checked, not just Home.
  await boot(page);
  const clashes = await page.evaluate(async () => {
    const { TABS, QAT_KEYTIPS, TOP_KEYTIPS } = await import("/ribbon.model.js");
    const out = [];
    const check = (where, keys) => {
      for (const a of keys) for (const b of keys) {
        if (a.ref !== b.ref && b.key.startsWith(a.key)) out.push(`${where}: ${a.key} (${a.ref}) / ${b.key} (${b.ref})`);
      }
    };
    check("top", [{ key: "F", ref: "file" }, ...Object.entries(TOP_KEYTIPS || {}).map(([ref, key]) => ({ key, ref })),
      ...TABS.map((t) => ({ key: t.key, ref: t.id })), ...(QAT_KEYTIPS || []).map((key, i) => ({ key, ref: `qat${i}` }))]);
    for (const tab of TABS) {
      const keys = [];
      const walk = (it) => {
        if (it.kind === "rows") { it.rows.forEach((r) => r.forEach(walk)); return; }
        keys.push({ key: it.tip, ref: it.id });
      };
      for (const g of tab.groups) {
        g.items.forEach(walk);
        if (g.launcher) keys.push({ key: g.launcherTip, ref: `${g.label} launcher` });
        keys.push({ key: g.tip, ref: `${g.label} group` });
      }
      for (const k of keys) if (!k.key) out.push(`${tab.id}: ${k.ref} has no KeyTip`);
      check(tab.id, keys.filter((k) => k.key));
    }
    return out;
  });
  expect(clashes).toEqual([]);
});

test("a folded group's KeyTip opens it and badges what is inside", async ({ page }) => {
  await boot(page);
  // Narrow enough that Home folds its lowest-ranked groups into one button.
  await page.setViewportSize({ width: 900, height: 800 });
  await expect(page.locator(".rb-panel:not([hidden]) .rb-group.is-folded").first()).toBeAttached();
  await page.locator("#grid").focus();
  await alt(page);
  await page.keyboard.press("h");
  const top = await shown(page);
  const [key, target] = Object.entries(top).find(([, t]) => t.startsWith("group:")) || [];
  expect(key, `no folded group's KeyTip among ${JSON.stringify(top)}`).toBeTruthy();
  for (const ch of key) await page.keyboard.press(ch.toLowerCase());
  await expect(page.locator(".rb-fold-pop")).toBeVisible();
  // The level is now the group's contents, badged inside the popover.
  const inside = await page.evaluate(() => [...document.querySelectorAll(".oc-ribbon .rb-tip")].map((t) => t.dataset.target));
  const contents = await page.locator(".rb-fold-pop [data-keytip]").evaluateAll((ns) =>
    ns.filter((n) => n.getClientRects().length > 0).map((n) => n.dataset.cmd));
  expect(contents.length, `${target} opened empty`).toBeGreaterThan(0);
  expect(inside.sort()).toEqual(contents.sort());
  // Escape closes the group and goes back to the tab's level.
  await page.keyboard.press("Escape");
  await expect(page.locator(".rb-fold-pop")).toHaveCount(0);
  await expect.poll(() => shown(page)).toMatchObject({ [key]: target });
});
