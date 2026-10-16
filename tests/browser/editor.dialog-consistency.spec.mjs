// Dialogs and side panels share one design (UX-SHELL-18).
//
// Owner, 2026-10-14: "not just toolbar but also make sure dialog, sidepanel,
// properties all are consistant". Measured before: eight dialog widths from
// 209 to 510px, four button styles (blue/outlined, native-looking, grey
// full-width, a grey "Save a version"), Cancel before the action, and a text
// "×" for close. This gate opens every dialog and panel and holds each to the
// same frame, so the next one built cannot drift.

import fs from "node:fs";
import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
}

const DIALOGS = [
  "format.format-cells", "formulas.insert-function", "file.properties", "data.sort-range.custom-sort",
  "insert.table", "format.row-height", "insert.hyperlink", "data.text-to-columns",
  "data.remove-duplicates", "help.about-opencalc", "edit.go-to", "edit.go-to-special", "tools.name-manager", "formulas.define-name", "format.protection.protect-this-sheet",
  // Find and Replace (XL-UX-024/025): modeless, but the same frame.
  "edit.find-replace", "edit.replace",
  "data.goal-seek",
  // The Macros window (MACRO-02).
  "view.macros",
  // File ▸ Options ▸ Trust Center (MACRO-13).
  "file.trust-center",
];
const PANELS = ["data.data-validation", "file.page-setup", "insert.note", "file.version-history", "insert.pivottable"];

const measure = (page, root) => page.evaluate((root) => {
  const box = document.querySelector(root);
  const r = box.getBoundingClientRect();
  const buttons = [...box.querySelectorAll(".oc-confirm-actions button, .panel-actions button, .hist-actions button")]
    .filter((b) => b.offsetParent);
  const close = document.querySelector(root === ".oc-modal-box" ? "#oc-modal-x" : "#side-panel-close");
  return {
    width: r.width,
    radius: getComputedStyle(box).borderTopLeftRadius,
    buttons: buttons.map((b) => ({
      cls: b.className, h: Math.round(b.getBoundingClientRect().height), primary: b.classList.contains("primary"),
    })),
    closeIsIcon: !!close?.querySelector("svg") && close.textContent.trim() === "",
  };
}, root);

for (const id of DIALOGS) {
  test(`dialog ${id} uses the shared frame`, async ({ page }) => {
    await boot(page);
    await page.evaluate((id) => window.opencalcEditor.runCommand(id), id);
    await expect(page.locator("#oc-modal")).toBeVisible();
    const m = await measure(page, ".oc-modal-box");
    expect(m.radius).toBe("8px");
    expect(m.width, "narrower than the shared minimum").toBeGreaterThanOrEqual(359);
    expect(m.width, "wider than the shared maximum").toBeLessThanOrEqual(561);
    expect(m.closeIsIcon, "close is drawn as text").toBe(true);
    for (const b of m.buttons) {
      expect(b.cls, "a footer button outside the shared component").toMatch(/\boc-btn\b/);
      expect(b.h).toBe(32);
    }
    const firstPrimary = m.buttons.findIndex((b) => b.primary);
    if (firstPrimary >= 0) expect(firstPrimary, "the action is not first (Excel: OK, then Cancel)").toBe(0);
  });
}

// Edit Hyperlink (XL-UX-164) is Insert Hyperlink prefilled, with Excel's
// Remove Link added to the footer — a third button that must not break the
// frame or take OK's place as the first action.
test("dialog Edit Hyperlink uses the shared frame", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_set_hyperlink(0, 0, 0, "https://example.com/", "", "", "");
    ed.selectForTest(0, 0);
    ed.runCommand("insert.hyperlink");
  });
  await expect(page.locator("#oc-modal-title")).toHaveText("Edit Hyperlink");
  const m = await measure(page, ".oc-modal-box");
  expect(m.radius).toBe("8px");
  expect(m.width).toBeGreaterThanOrEqual(359);
  expect(m.width).toBeLessThanOrEqual(561);
  expect(m.buttons.map((b) => b.cls)).toEqual(["oc-btn primary", "oc-btn", "oc-btn oc-link-remove"]);
  for (const b of m.buttons) expect(b.h).toBe(32);
});

for (const id of PANELS) {
  test(`panel ${id} uses the shared frame`, async ({ page }) => {
    await boot(page);
    await page.evaluate((id) => window.opencalcEditor.runCommand(id), id);
    await expect(page.locator("#side-panel")).toBeVisible();
    const m = await measure(page, "#side-panel");
    expect(m.closeIsIcon, "close is drawn as text").toBe(true);
    for (const b of m.buttons) {
      expect(b.cls, "a panel button outside the shared component").toMatch(/\boc-btn\b/);
      expect(b.h).toBe(32);
    }
    const firstPrimary = m.buttons.findIndex((b) => b.primary);
    if (firstPrimary >= 0) expect(firstPrimary, "the action is not first").toBe(0);
  });
}

// Visual Basic (MACRO-05) exists only for a workbook carrying a VBA project, so it
// is opened on one rather than on the demo.
test("dialog developer.visual-basic uses the shared frame", async ({ page }) => {
  await boot(page);
  const bytes = [...fs.readFileSync(new URL("../fixtures/vba-hello.xlsm", import.meta.url))];
  expect(await page.evaluate((b) => window.opencalcEditor.openBytes(new Uint8Array(b), "vba-hello.xlsm"), bytes)).toBe(true);
  await page.evaluate(() => window.opencalcEditor.runCommand("developer.visual-basic"));
  await expect(page.locator("#oc-modal")).toBeVisible();
  const m = await measure(page, ".oc-modal-box");
  expect(m.radius).toBe("8px");
  expect(m.width, "narrower than the shared minimum").toBeGreaterThanOrEqual(359);
  expect(m.width, "wider than the shared maximum").toBeLessThanOrEqual(561);
  expect(m.closeIsIcon, "close is drawn as text").toBe(true);
  for (const b of m.buttons) {
    expect(b.cls, "a footer button outside the shared component").toMatch(/\boc-btn\b/);
    expect(b.h).toBe(32);
  }
  const firstPrimary = m.buttons.findIndex((b) => b.primary);
  if (firstPrimary >= 0) expect(firstPrimary, "the action is not first (Excel: OK, then Cancel)").toBe(0);
});
