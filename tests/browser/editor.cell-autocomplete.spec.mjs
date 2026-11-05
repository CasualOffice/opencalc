// AutoComplete for cell values (XL-UX-017).
//
// Excel ("Automatically complete data entries"): typing the first characters of
// an entry already in the column fills in the rest *in the cell*, with the
// completed part selected, so the next keystroke either accepts it (Enter, Tab,
// an arrow) or overwrites it. It only completes text, only from the entries in
// the contiguous block of the column above and below the cell, and only once
// the typed characters pick out one entry. Matching ignores case; accepting
// takes the entry's own case.
//
// Every assertion is on the *in-cell* editor and on what the cell stores after
// Enter — the two places the user sees the feature — not on internal state.

import { expect, test } from "@playwright/test";

async function boot(page, cells) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate((cells) => {
    const ed = window.opencalcEditor;
    const api = ed.wasmApi();
    api.session_new();
    for (const [r, c, v] of cells) api.session_set_cell(0, r, c, v);
    ed.selectForTest(0, 0);
  }, cells);
}

/// Select a cell and give the grid the keyboard, as a click would.
async function at(page, row, col) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c), [row, col]);
  await page.locator("#grid").focus();
}

/// The in-cell editor's text and its selected range.
const editor = (page) => page.evaluate(() => {
  const t = document.getElementById("inline-edit");
  return { value: t.value, from: t.selectionStart, to: t.selectionEnd };
});

const stored = (page, row, col) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [row, col]);

// B1:B3, a header and two entries, with the cell under them to type in.
const COLUMN = [[0, 1, "Item"], [1, 1, "Widget"], [2, 1, "Gadget"]];

test("typing the start of an entry above completes it with the tail selected", async ({ page }) => {
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("Wid");
  expect(await editor(page), "the rest of Widget is filled in and selected")
    .toEqual({ value: "Widget", from: 3, to: 6 });
  await page.keyboard.press("Enter");
  expect(await stored(page, 3, 1)).toBe("Widget");
});

test("the first character alone completes when it picks out one entry", async ({ page }) => {
  // The opening keystroke starts the edit through the grid, not through the
  // editor's own input event — the path most easily left uncompleted.
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("G");
  expect(await editor(page)).toEqual({ value: "Gadget", from: 1, to: 6 });
});

test("matching ignores case and accepting keeps the entry's case", async ({ page }) => {
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("wid");
  expect((await editor(page)).value.toLowerCase()).toBe("widget");
  await page.keyboard.press("Enter");
  expect(await stored(page, 3, 1), "the entry is stored as the column already spells it").toBe("Widget");
});

test("Backspace removes the suggestion and keeps what was typed", async ({ page }) => {
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("Wid");
  await page.keyboard.press("Backspace");
  expect(await editor(page), "deleting the selected tail must not complete it again")
    .toEqual({ value: "Wid", from: 3, to: 3 });
  await page.keyboard.press("Enter");
  expect(await stored(page, 3, 1)).toBe("Wid");
});

test("Delete removes the suggestion too", async ({ page }) => {
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("Gad");
  await page.keyboard.press("Delete");
  expect((await editor(page)).value).toBe("Gad");
});

test("typing over the suggestion replaces it", async ({ page }) => {
  // The tail is selected precisely so the next character overwrites it.
  await boot(page, COLUMN);
  await at(page, 3, 1);
  await page.keyboard.type("Widx");
  await page.keyboard.press("Enter");
  expect(await stored(page, 3, 1)).toBe("Widx");
});

test("nothing completes until the typed characters pick out one entry", async ({ page }) => {
  await boot(page, [[0, 1, "Widget"], [1, 1, "Widow"]]);
  await at(page, 2, 1);
  await page.keyboard.type("Wid");
  expect((await editor(page)).value, "two entries start with Wid").toBe("Wid");
  await page.keyboard.type("g");
  expect(await editor(page)).toEqual({ value: "Widget", from: 4, to: 6 });
});

test("numbers and formulas are never completed", async ({ page }) => {
  await boot(page, [[0, 1, "12345"], [1, 1, "Sum of all"], [2, 1, "=SUM(A1:A2)"]]);
  await at(page, 3, 1);
  await page.keyboard.type("123");
  expect((await editor(page)).value, "a number is not an entry to complete to").toBe("123");
  await page.keyboard.press("Escape");
  await at(page, 3, 1);
  await page.keyboard.type("=Su");
  // A formula is the function AutoComplete's, not this one's: the cell text
  // must stay what was typed, whatever the function list does.
  expect((await editor(page)).value).toBe("=Su");
});

test("only the contiguous block of the column is searched, above and below", async ({ page }) => {
  // B1 is cut off from B4 by the blank in B2; B6 is joined to it through B5.
  await boot(page, [[0, 1, "Apple"], [3, 1, "Banana"], [5, 1, "Cherry"], [4, 1, "Damson"]]);
  await at(page, 2, 1);
  await page.keyboard.type("A");
  expect((await editor(page)).value, "an entry beyond a blank is another table").toBe("A");
  await page.keyboard.press("Escape");
  // B3 has the blank B2 above it and the run B4:B6 below it.
  await at(page, 2, 1);
  await page.keyboard.type("Ch");
  expect((await editor(page)).value, "an entry below the cell completes").toBe("Cherry");
});

// Microsoft: "Entries that contain only numbers, dates, or times are not
// completed." A date or time edits as its typed form ("1/5/2026", "9:30"), and
// a boolean as "TRUE", so a filter on the input string's numeric value lets all
// of them through — and in a date column, typing 1 and pressing Enter stored
// the date 1/5/2026 instead of the number 1. What counts is the stored value's
// type, which only the engine knows.
for (const [what, entry, typed] of [
  ["a date", "1/5/2026", "1"],
  ["a time", "9:30", "9"],
  ["an ISO date", "2026-01-05", "2"],
  ["a boolean", "TRUE", "t"],
  // No error case: typing "#N/A" stores text here, not the error value Excel
  // stores, so an error can only arrive from a file. The engine's kind check
  // leaves errors out by construction.
]) {
  test(`${what} is never completed, and the typed value is what is stored`, async ({ page }) => {
    await boot(page, [[0, 1, entry]]);
    await at(page, 1, 1);
    await page.keyboard.type(typed);
    expect((await editor(page)).value, `${entry} is not text to complete to`).toBe(typed);
    await page.keyboard.press("Enter");
    expect(await stored(page, 1, 1)).toBe(typed);
  });
}

test("a text entry that starts with a digit still completes", async ({ page }) => {
  // Only entries that are *only* a number, date or time are left out.
  await boot(page, [[0, 1, "1st Floor"]]);
  await at(page, 1, 1);
  await page.keyboard.type("1s");
  expect(await editor(page)).toEqual({ value: "1st Floor", from: 2, to: 9 });
});

test("the Alt+Down pick list offers the column's text and nothing else", async ({ page }) => {
  // The pick list reads the same entries, so it must leave out the same values.
  await boot(page, [[0, 1, "Widget"], [1, 1, "1/5/2026"], [2, 1, "9:30"], [3, 1, "TRUE"], [4, 1, "42"]]);
  await at(page, 5, 1);
  await page.keyboard.press("Alt+ArrowDown");
  await expect(page.locator("#sheet-ctx button")).toHaveText(["Widget"]);
});
