// Copy and hidden rows and columns (XL-UX-176).
//
// Excel copies a row or column hidden **by hand** along with the visible ones:
// "By default, Excel copies hidden or filtered cells in addition to visible
// cells" (support.microsoft.com, "Copy visible cells only"). Rows an
// AutoFilter has filtered out are the exception in Excel for Microsoft 365 on
// Windows — a plain Ctrl+C of a filtered list copies only what the filter
// shows (the Microsoft page's "or filtered" is disputed on Microsoft's own
// Tech Community, "Wrong official information 'Copy visible cells only'"). To
// copy only what is on screen in any other case, Go To Special ▸ Visible cells
// only first.
//
// OpenCalc skipped every hidden row and column, so A1:A4 with row 2 hidden by
// hand pasted three rows.

import { expect, test } from "@playwright/test";

///        A      B
///   1    one    1
///   2    two    2
///   3    three  3
///   4    four   4
async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    ["one", "two", "three", "four"].forEach((v, r) => {
      a.session_set_cell(0, r, 0, v);
      a.session_set_cell(0, r, 1, String(r + 1));
    });
    // What reached the system clipboard, as text.
    window.__copied = null;
    const put = async (t) => { window.__copied = t; };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: put,
        write: async (items) => { window.__copied = await (await items[0].getType("text/plain")).text(); },
        readText: async () => window.__copied ?? "",
      },
    });
    window.opencalcEditor.draw();
  });
  await page.locator("#grid").focus();
}

const select = (page, r0, c0, r1, c1) => page.evaluate(([r0, c0, r1, c1]) => {
  window.opencalcEditor.selectForTest(r0, c0);
  window.opencalcEditor.extendSelectionForTest(r1, c1);
}, [r0, c0, r1, c1]);
const copy = async (page) => {
  await page.evaluate(() => window.opencalcEditor.runCommand("edit.copy"));
  await expect.poll(() => page.evaluate(() => window.__copied)).not.toBeNull();
  return page.evaluate(() => window.__copied);
};
/// Paste the app's own clipboard with its top-left at (row, col), and read the
/// column back.
const pasteAndRead = (page, row, col, n) => page.evaluate(([row, col, n]) => {
  const a = window.opencalcEditor.wasmApi();
  a.session_clip_paste(0, row, col);
  return Array.from({ length: n }, (_, i) => a.session_cell_input(0, row + i, col));
}, [row, col, n]);

test("a row hidden by hand is copied with the rest", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_hide_rows(0, 1, 1));
  await select(page, 0, 0, 3, 0);   // A1:A4
  expect(await copy(page)).toBe("one\ntwo\nthree\nfour\n");
  expect(await pasteAndRead(page, 0, 3, 4)).toEqual(["one", "two", "three", "four"]);
});

test("a column hidden by hand is copied with the rest", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_hide_cols(0, 0, 0));
  await select(page, 0, 0, 1, 1);   // A1:B2, A hidden
  expect(await copy(page)).toBe("one\t1\ntwo\t2\n");
});

test("rows a filter hides are left out, as in Excel", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_insert_rows(0, 0, 1);
    a.session_set_cell(0, 0, 0, "Name");
    a.session_set_cell(0, 0, 1, "N");
    a.session_set_filter_range(0, 0, 0, 4, 1);
    a.session_set_filter_values(0, 0, ["one", "three"]);
  });
  await select(page, 0, 0, 4, 0);   // A1:A5, "two" and "four" filtered out
  expect(await copy(page)).toBe("Name\none\nthree\n");
});

test("Go To Special ▸ Visible cells only still copies only what is shown", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_hide_rows(0, 1, 1));
  await select(page, 0, 0, 3, 1);   // A1:B4
  await page.evaluate(() => window.opencalcEditor.runCommand("edit.go-to-special"));
  await page.locator("#oc-modal .oc-radio", { hasText: /^Visible cells only$/ }).locator("input").check();
  await page.locator("#oc-modal .oc-btn.primary").click();
  expect(await copy(page)).toBe("one\t1\nthree\t3\nfour\t4\n");
  expect(await pasteAndRead(page, 0, 4, 3)).toEqual(["one", "three", "four"]);
});
