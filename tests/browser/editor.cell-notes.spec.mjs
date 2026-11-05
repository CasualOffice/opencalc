// Notes are notes, comments are comments (XL-UX-039).
//
// Excel has two things a cell can carry. A **note** is the sticky-note box:
// pale yellow, the author's name at the top, typed into in place beside the
// cell, shown on hover behind a red corner mark. A **comment** is a thread with
// replies, in the Comments pane. Microsoft's own steps ("Insert comments and
// notes in Excel"): right-click the cell and select New Note (or press
// Shift+F2), type, click outside the cell; Edit Note and Delete Note on the
// same menu; "Notes don't have a Reply box."
//
// Here Shift+F2 and New Note opened the threaded pane, and what it saved
// carried a timestamp — a threaded comment, written to the 2018 parts, for a
// user who asked for a note. The model already tells the two apart (a note is
// a `CellComment` with no timestamp, no replies and not resolved), so these
// specs check what is *stored* as well as what is shown.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    localStorage.setItem("oc-comment-author", "Ada Lovelace");
    const ed = window.opencalcEditor;
    ed.wasmApi().session_new();
    ed.selectForTest(1, 1); // B2
  });
  await page.locator("#grid").focus();
}

const thread = (page, row, col) =>
  page.evaluate(([r, c]) => JSON.parse(window.opencalcEditor.wasmApi().session_comment_thread(0, r, c)), [row, col]);

const NOTE = ".oc-note-editor";

/// Where a cell is on screen, in page pixels.
const cellBox = (page, row, col) => page.evaluate(([r, c]) => {
  const b = window.opencalcEditor.cellBoxForTest(r, c);
  const g = document.getElementById("grid").getBoundingClientRect();
  const z = window.opencalcEditor.state.zoom || 1;
  return { x: g.left + b.x * z, y: g.top + b.y * z, w: b.w * z, h: b.h * z };
}, [row, col]);

test("Shift+F2 opens a note box beside the cell, not the Comments pane", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Shift+F2");
  const box = page.locator(NOTE);
  await expect(box, "Shift+F2 opened no note box").toBeVisible();
  await expect(page.locator("#side-panel"), "the threaded pane is not a note").toBeHidden();
  // The author heads the note, as Excel writes its user name at the top.
  await expect(box.locator(".oc-note-author")).toHaveText("Ada Lovelace:");
  // Anchored to the right of its cell, top level with it.
  const cell = await cellBox(page, 1, 1);
  const b = await box.boundingBox();
  expect(b.x, "the note opens to the right of its cell").toBeGreaterThanOrEqual(cell.x + cell.w);
  expect(b.x - (cell.x + cell.w), "and close to it").toBeLessThan(40);
  expect(Math.abs(b.y - cell.y), "level with the cell").toBeLessThan(24);
  await expect(box.locator("textarea"), "typing goes straight into the note").toBeFocused();
});

test("typing into the note and clicking outside saves a note, not a thread", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Shift+F2");
  await page.keyboard.type("Check against the invoice");
  // Microsoft's step: "Click outside the cell."
  const far = await cellBox(page, 8, 6);
  await page.mouse.click(far.x + 5, far.y + 5);
  await expect(page.locator(NOTE)).toBeHidden();
  const t = await thread(page, 1, 1);
  expect(t, "nothing was saved").not.toBeNull();
  expect(t.text).toBe("Check against the invoice");
  expect(t.author).toBe("Ada Lovelace");
  // The three things that make a thread. Any one of them sends it to the
  // threaded-comment parts on save; a note has none.
  expect(t.created, "a note carries no timestamp").toBeNull();
  expect(t.replies).toEqual([]);
  expect(t.resolved).toBe(false);
});

test("an empty note box leaves no note behind", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Shift+F2");
  await page.keyboard.press("Escape");
  await expect(page.locator(NOTE)).toBeHidden();
  expect(await thread(page, 1, 1)).toBeNull();
});

test("Shift+F2 on a cell with a note edits it in place", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_comment(0, 1, 1, "first draft", "Grace", ""));
  await page.keyboard.press("Shift+F2");
  const ta = page.locator(`${NOTE} textarea`);
  await expect(ta).toHaveValue("first draft");
  await expect(page.locator(`${NOTE} .oc-note-author`), "the note keeps who wrote it").toHaveText("Grace:");
  await ta.fill("second draft");
  await page.keyboard.press("Escape");
  const t = await thread(page, 1, 1);
  expect(t.text).toBe("second draft");
  expect(t.author).toBe("Grace");
  expect(t.created).toBeNull();
});

test("Review ▸ New Note opens the note box and New Comment the Comments pane", async ({ page }) => {
  await boot(page);
  expect(await page.evaluate(() => window.opencalcEditor.runCommand("insert.note"))).toBe(true);
  await expect(page.locator(NOTE)).toBeVisible();
  await expect(page.locator("#side-panel")).toBeHidden();
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => window.opencalcEditor.runCommand("insert.comment"))).toBe(true);
  await expect(page.locator("#side-panel")).toBeVisible();
  await expect(page.locator("#side-panel-title")).toHaveText(/comment/i);
  await expect(page.locator(NOTE)).toBeHidden();
});

test("the ribbon offers New Comment and New Note as separate controls", async ({ page }) => {
  await boot(page);
  const ribbon = await page.evaluate(async () => {
    const { TABS: RIBBON } = await import("./ribbon.model.js");
    const review = RIBBON.find((t) => t.id === "review");
    const insert = RIBBON.find((t) => t.id === "insert");
    const flat = (tab) => JSON.stringify(tab.groups);
    return { review: flat(review), insert: flat(insert) };
  });
  expect(ribbon.review).toContain('"insert.comment"');
  expect(ribbon.review).toContain('"insert.note"');
  // Insert ▸ Comments ▸ Comment is Excel's threaded comment.
  expect(ribbon.insert).toContain('"insert.comment"');
});

test("the context menu offers New Comment and New Note on a plain cell", async ({ page }) => {
  await boot(page);
  const cell = await cellBox(page, 1, 1);
  await page.mouse.click(cell.x + 5, cell.y + 5, { button: "right" });
  const menu = page.locator("#sheet-ctx");
  await expect(menu.getByText("New Comment", { exact: true })).toBeVisible();
  await menu.getByText("New Note", { exact: true }).click();
  await expect(page.locator(NOTE)).toBeVisible();
  await expect(page.locator("#side-panel")).toBeHidden();
});

test("the context menu edits and deletes a note", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_comment(0, 1, 1, "remember", "", ""));
  const cell = await cellBox(page, 1, 1);
  await page.mouse.click(cell.x + 5, cell.y + 5, { button: "right" });
  const menu = page.locator("#sheet-ctx");
  await expect(menu.getByText("New Note", { exact: true }), "a cell holds one note").toHaveCount(0);
  await menu.getByText("Edit Note", { exact: true }).click();
  await expect(page.locator(`${NOTE} textarea`)).toHaveValue("remember");
  await page.keyboard.press("Escape");
  await page.mouse.click(cell.x + 5, cell.y + 5, { button: "right" });
  await page.locator("#sheet-ctx").getByText("Delete Note", { exact: true }).click();
  expect(await thread(page, 1, 1)).toBeNull();
});

test("hovering a note shows it as a note, with its author", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_comment(0, 1, 1, "remember", "Grace", "");
    window.opencalcEditor.selectForTest(1, 1); // redraws, which collects the marked cells
  });
  const cell = await cellBox(page, 1, 1);
  await page.mouse.move(cell.x + cell.w / 2, cell.y + cell.h / 2);
  const tip = page.locator("#comment-tip");
  await expect(tip).toBeVisible();
  await expect(tip).toHaveClass(/\boc-note-tip\b/);
  await expect(tip.locator(".oc-note-author")).toHaveText("Grace:");
  await expect(tip).toContainText("remember");
});

test("Shift+F2 on a threaded comment opens its thread rather than turning it into a note", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi()
    .session_set_comment(0, 1, 1, "Is this right?", "Grace", "2026-01-02T03:04:05.00"));
  await page.keyboard.press("Shift+F2");
  await expect(page.locator("#side-panel")).toBeVisible();
  await expect(page.locator(NOTE)).toBeHidden();
  expect((await thread(page, 1, 1)).created, "the thread is untouched").not.toBeNull();
});
