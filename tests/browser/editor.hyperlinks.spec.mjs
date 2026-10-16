// Hyperlinks followed as Excel follows them (XL-FEAT-139), and Excel's
// Insert Hyperlink dialog (XL-UX-164).
//
// `followHyperlink` read a link's location as a bare cell reference: a link to
// a defined name went nowhere, a link to a sheet that does not exist selected
// the cell on the *current* sheet, a range selected only its corner and a
// quoted sheet name holding `!` split in the wrong place. `HYPERLINK()` cells
// were not links at all, because only stored hyperlinks were asked about.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    const a = ed.wasmApi();
    a.session_new();
    a.session_add_sheet(); // Sheet2
    a.session_add_sheet(); // → "My ! Sheet"
    a.session_rename_sheet(2, "My ! Sheet");
    a.session_add_sheet(); // → "It's"
    a.session_rename_sheet(3, "It's");
    // Every window.open is recorded rather than performed, so a test can see
    // what would have been opened — and that nothing was, when it must not be.
    window.__opened = [];
    window.open = (...args) => { window.__opened.push(args); return null; };
    ed.switchSheet(0);
    ed.renderTabs();
    ed.selectForTest(0, 0);
    ed.draw();
  });
}

const ed = (page, fn, arg) => page.evaluate(fn, arg);
const sel = (page) => ed(page, () => window.opencalcEditor.selectionRectForTest());
const sheet = (page) => ed(page, () => window.opencalcEditor.personalViewForTest().sheet);
const opened = (page) => ed(page, () => window.__opened);

/// A stored hyperlink on Sheet1's cell, through the engine.
const link = (page, row, col, target, location) =>
  ed(page, ([r, c, t, l]) => {
    const e = window.opencalcEditor;
    e.wasmApi().session_set_cell(0, r, c, "link");
    e.wasmApi().session_set_hyperlink(0, r, c, t, l, "", "");
    e.draw();
  }, [row, col, target, location]);

/// Where a cell is on screen, in page coordinates.
const cellCenter = (page, r, c) => ed(page, ([r, c]) => {
  const b = window.opencalcEditor.cellBoxForTest(r, c);
  const g = document.getElementById("grid").getBoundingClientRect();
  return { x: g.left + b.x + b.w / 2, y: g.top + b.y + b.h / 2 };
}, [r, c]);

/// A plain, quick click — Excel's "follow this link".
async function clickCell(page, r, c) {
  const p = await cellCenter(page, r, c);
  await page.mouse.click(p.x, p.y);
}

test.describe("following a link (XL-FEAT-139)", () => {
  test("a link to a defined name selects the name's range, switching sheet", async ({ page }) => {
    await boot(page);
    await ed(page, () => window.opencalcEditor.wasmApi().session_define_name("Totals", "Sheet2!$B$3:$D$5"));
    // Excel's names are case-insensitive, and a link's location is no exception.
    await link(page, 1, 1, "", "totals");
    await clickCell(page, 1, 1);
    expect(await sheet(page)).toBe(1);
    expect(await sel(page)).toEqual({ r0: 2, c0: 1, r1: 4, c1: 3 });
  });

  test("a sheet-scoped name, written Sheet!Name, resolves in its sheet's scope", async ({ page }) => {
    await boot(page);
    await ed(page, () => {
      const a = window.opencalcEditor.wasmApi();
      // Two names spelled alike: the workbook one must not answer for Sheet2!Local.
      a.session_define_name("Local", "Sheet1!$H$8");
      a.session_set_name("", -1, "Local", 1, "Sheet2!$E$2:$E$9", "");
    });
    await link(page, 1, 1, "", "Sheet2!Local");
    await clickCell(page, 1, 1);
    expect(await sheet(page)).toBe(1);
    expect(await sel(page)).toEqual({ r0: 1, c0: 4, r1: 8, c1: 4 });
  });

  test("a link to a sheet that does not exist says so and does not move", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "", "Nowhere!C7");
    await clickCell(page, 1, 1);
    await expect(page.locator("#tb-status")).toContainText("Reference isn't valid.");
    expect(await sheet(page)).toBe(0);
    // The click selects the link's own cell; the link must not move it on.
    expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
  });

  test("a range location selects the whole range, active cell at its top-left", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "", "Sheet2!C3:E6");
    await clickCell(page, 1, 1);
    expect(await sheet(page)).toBe(1);
    expect(await sel(page)).toEqual({ r0: 2, c0: 2, r1: 5, c1: 4 });
    // The name box names the active cell, which Excel puts at the top-left.
    await expect(page.locator("#cell-ref")).toHaveValue("C3");
  });

  for (const [location, index, rect] of [
    ["'My ! Sheet'!A1", 2, { r0: 0, c0: 0, r1: 0, c1: 0 }],
    ["'It''s'!B2", 3, { r0: 1, c0: 1, r1: 1, c1: 1 }],
  ]) {
    test(`a quoted sheet name parses: ${location}`, async ({ page }) => {
      await boot(page);
      await link(page, 1, 1, "", location);
      await clickCell(page, 1, 1);
      expect(await sheet(page)).toBe(index);
      expect(await sel(page)).toEqual(rect);
    });
  }

  test("a HYPERLINK() cell to a place in the workbook is followed", async ({ page }) => {
    await boot(page);
    await ed(page, () => {
      const e = window.opencalcEditor;
      e.wasmApi().session_set_cell(0, 2, 2, '=HYPERLINK("#Sheet2!D4","Go")');
      e.draw();
    });
    await clickCell(page, 2, 2);
    expect(await sheet(page)).toBe(1);
    expect(await sel(page)).toEqual({ r0: 3, c0: 3, r1: 3, c1: 3 });
  });

  test("a HYPERLINK() whose target is computed follows the computed target", async ({ page }) => {
    await boot(page);
    await ed(page, () => {
      const e = window.opencalcEditor;
      const a = e.wasmApi();
      // A formula, because typed text starting with `'` is a quote prefix.
      a.session_set_cell(0, 0, 5, `="'It''s'!F9"`);
      a.session_set_cell(0, 2, 2, '=HYPERLINK("#"&F1,"Go")');
      e.draw();
    });
    await clickCell(page, 2, 2);
    expect(await sheet(page)).toBe(3);
    expect(await sel(page)).toEqual({ r0: 8, c0: 5, r1: 8, c1: 5 });
  });

  test("a HYPERLINK() to a web page opens it in a new, unlinked window", async ({ page }) => {
    await boot(page);
    await ed(page, () => {
      const e = window.opencalcEditor;
      e.wasmApi().session_set_cell(0, 2, 2, '=HYPERLINK("https://example.com/a?b=1","site")');
      e.draw();
    });
    await clickCell(page, 2, 2);
    expect(await opened(page)).toEqual([["https://example.com/a?b=1", "_blank", "noopener,noreferrer"]]);
  });

  for (const target of [
    "javascript:alert(1)",
    " JavaScript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "file:///etc/passwd",
    "vbscript:msgbox(1)",
  ]) {
    test(`a link to ${target.trim().split(":")[0]}: is refused, stored or computed`, async ({ page }) => {
      await boot(page);
      await link(page, 1, 1, target, "");
      await clickCell(page, 1, 1);
      await ed(page, ([t]) => {
        const e = window.opencalcEditor;
        e.wasmApi().session_set_cell(0, 3, 3, `=HYPERLINK("${t.replace(/"/g, '""')}","x")`);
        e.draw();
      }, [target]);
      await clickCell(page, 3, 3);
      expect(await opened(page), "nothing may be opened").toEqual([]);
      await expect(page.locator("#tb-status")).toContainText("Cannot open the specified file.");
    });
  }

  test("mailto: links open", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "mailto:a@example.com?subject=Hi", "");
    await clickCell(page, 1, 1);
    expect(await opened(page)).toEqual([["mailto:a@example.com?subject=Hi", "_blank", "noopener,noreferrer"]]);
  });

  test("a click on a link that is already selected follows it again, as Excel does", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "https://example.com/", "");
    await ed(page, () => window.opencalcEditor.selectForTest(1, 1));
    await clickCell(page, 1, 1);
    expect(await opened(page)).toHaveLength(1);
  });

  test("click-and-hold selects the link's cell without following it", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "https://example.com/", "");
    const p = await cellCenter(page, 1, 1);
    await page.mouse.move(p.x, p.y);
    await page.mouse.down();
    await page.waitForTimeout(1200);
    await page.mouse.up();
    expect(await opened(page)).toEqual([]);
    expect(await sel(page)).toEqual({ r0: 1, c0: 1, r1: 1, c1: 1 });
  });

  test("Ctrl+click does not follow a link (it is not Excel's gesture)", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "https://example.com/", "");
    const p = await cellCenter(page, 1, 1);
    await page.keyboard.down("Control");
    await page.mouse.click(p.x, p.y);
    await page.keyboard.up("Control");
    expect(await opened(page)).toEqual([]);
  });

  test("HYPERLINK() cells are drawn as links", async ({ page }) => {
    await boot(page);
    const cells = await ed(page, () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_set_cell(0, 2, 2, '=HYPERLINK("#Sheet2!A1","Go")');
      a.session_set_cell(0, 3, 3, "=1+1");
      return JSON.parse(a.session_hyperlink_cells(0, 0, 0, 10, 10));
    });
    expect(cells).toEqual([{ r: 2, c: 2 }]);
  });
});

// ---- Excel's Insert Hyperlink dialog (XL-UX-164) -----------------------------
//
// Reference: support.microsoft.com, "Work with links in Excel". Ours was two
// free text boxes; Excel's has a "Link to:" rail, Text to display, ScreenTip…,
// and a pane per kind of destination.

const linkAt = (page, r, c, s = 0) =>
  ed(page, ([s, r, c]) => JSON.parse(window.opencalcEditor.wasmApi().session_hyperlink_at(s, r, c)), [s, r, c]);
const cellText = (page, r, c) =>
  ed(page, ([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const modal = (page) => page.locator("#oc-modal");

async function openDialog(page, r, c) {
  await ed(page, ([r, c]) => window.opencalcEditor.selectForTest(r, c), [r, c]);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+k");
  await expect(modal(page)).toBeVisible();
}

test.describe("Insert Hyperlink dialog (XL-UX-164)", () => {
  test("Ctrl+K opens Excel's dialog: Link to rail, Text to display, ScreenTip", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await expect(page.locator("#oc-modal-title")).toHaveText("Insert Hyperlink");
    await expect(page.locator(".oc-link-rail")).toContainText("Link to:");
    await expect(page.locator(".oc-link-rail [role=tab]")).toHaveText([
      "Existing File or Web Page", "Place in This Document", "E-mail Address",
    ]);
    // Each rail entry carries an icon, as Excel's does — drawn, not a glyph.
    expect(await page.locator(".oc-link-rail [role=tab] svg use").count()).toBe(3);
    await expect(page.locator("label[for=oc-link-display]")).toHaveText("Text to display:");
    await expect(page.locator("#oc-link-tip-btn")).toHaveText("ScreenTip…");
    await expect(page.locator("label[for=oc-link-address]")).toHaveText("Address:");
    await expect(page.locator("#oc-link-address")).toBeFocused();
  });

  test("a web address: http:// is assumed, the text to display follows it", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-address").fill("example.com/docs");
    await expect(page.locator("#oc-link-display")).toHaveValue("example.com/docs");
    await page.keyboard.press("Enter");
    await expect(modal(page)).toBeHidden();
    const l = await linkAt(page, 1, 1);
    expect(l.target).toBe("http://example.com/docs");
    expect(await cellText(page, 1, 1)).toBe("example.com/docs");
  });

  test("an address that is not web or e-mail is refused inline, dialog kept", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-address").fill("javascript:alert(1)");
    await page.locator("#oc-modal .oc-btn.primary").click();
    await expect(modal(page)).toBeVisible();
    await expect(page.locator("#oc-link-address-error")).toBeVisible();
    await expect(page.locator("#oc-link-address")).toHaveValue("javascript:alert(1)");
    expect(await linkAt(page, 1, 1)).toBeNull();
  });

  test("place in this document, chosen from the tree", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-to-place").click();
    await expect(page.locator("label[for=oc-link-ref]")).toHaveText("Type the cell reference:");
    await expect(page.locator(".oc-link-tree-label")).toHaveText("Or select a place in this document:");
    const tree = page.locator("#oc-link-tree");
    await expect(tree.locator(".oc-link-node")).toHaveText(["Cell Reference", "Defined Names"]);
    await tree.getByRole("treeitem", { name: "Sheet2", exact: true }).click();
    await page.locator("#oc-link-ref").fill("c4");
    await page.locator("#oc-modal .oc-btn.primary").click();
    await expect(modal(page)).toBeHidden();
    expect((await linkAt(page, 1, 1)).location).toBe("Sheet2!C4");
    // …and the link it made goes there.
    await clickCell(page, 1, 1);
    expect(await sheet(page)).toBe(1);
    expect(await sel(page)).toEqual({ r0: 3, c0: 2, r1: 3, c1: 2 });
  });

  test("a quoted sheet and a defined name from the tree", async ({ page }) => {
    await boot(page);
    await ed(page, () => window.opencalcEditor.wasmApi().session_define_name("Totals", "Sheet2!$B$3:$D$5"));
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-to-place").click();
    const tree = page.locator("#oc-link-tree");
    await tree.getByRole("treeitem", { name: "It's", exact: true }).click();
    await page.locator("#oc-link-ref").fill("B2");
    await page.keyboard.press("Enter");
    expect((await linkAt(page, 1, 1)).location).toBe("'It''s'!B2");

    await openDialog(page, 2, 2);
    await page.locator("#oc-link-to-place").click();
    await tree.getByRole("treeitem", { name: "Totals", exact: true }).click();
    // A name is its own place: the cell reference does not apply to it.
    await expect(page.locator("#oc-link-ref")).toBeDisabled();
    await page.locator("#oc-modal .oc-btn.primary").click();
    expect((await linkAt(page, 2, 2)).location).toBe("Totals");
  });

  test("an invalid cell reference is answered inline in Excel's words", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-to-place").click();
    await page.locator("#oc-link-ref").fill("not a ref");
    await page.locator("#oc-modal .oc-btn.primary").click();
    await expect(modal(page)).toBeVisible();
    await expect(page.locator("#oc-link-ref-error")).toHaveText("Reference isn't valid.");
    await expect(page.locator("#oc-link-ref")).toHaveValue("not a ref");
    await expect(page.locator("#oc-link-ref")).toHaveAttribute("aria-invalid", "true");
    // One dialog: the message is not a box over it.
    expect(await page.locator(".oc-modal:visible, #oc-modal:visible").count()).toBe(1);
    expect(await linkAt(page, 1, 1)).toBeNull();
  });

  test("an e-mail address with a subject becomes an encoded mailto:", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-to-email").click();
    await expect(page.locator("label[for=oc-link-email]")).toHaveText("E-mail address:");
    await expect(page.locator("label[for=oc-link-subject]")).toHaveText("Subject:");
    await page.locator("#oc-link-email").fill("ops@example.com");
    await page.locator("#oc-link-subject").fill("Q3 report & plan?");
    await expect(page.locator("#oc-link-display")).toHaveValue("mailto:ops@example.com");
    await page.locator("#oc-modal .oc-btn.primary").click();
    const l = await linkAt(page, 1, 1);
    expect(l.target).toBe("mailto:ops@example.com?subject=Q3%20report%20%26%20plan%3F");
  });

  test("ScreenTip… opens its box inline, not a dialog over this one", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-address").fill("https://example.com/");
    await page.locator("#oc-link-tip-btn").click();
    await expect(page.locator("label[for=oc-link-tip]")).toHaveText("ScreenTip text:");
    await page.locator("#oc-link-tip").fill("Opens the site");
    await page.locator("#oc-modal .oc-btn.primary").click();
    expect((await linkAt(page, 1, 1)).tooltip).toBe("Opens the site");
  });

  test("Edit Hyperlink is the same dialog, prefilled, with Remove Link", async ({ page }) => {
    await boot(page);
    await ed(page, () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_set_cell(0, 1, 1, "Go there");
      a.session_set_hyperlink(0, 1, 1, "", "Sheet2!D7", "a tip", "Go there");
    });
    await openDialog(page, 1, 1);
    await expect(page.locator("#oc-modal-title")).toHaveText("Edit Hyperlink");
    await expect(page.locator("#oc-link-to-place")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#oc-link-ref")).toHaveValue("D7");
    await expect(page.locator("#oc-link-tree").getByRole("treeitem", { name: "Sheet2", exact: true }))
      .toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#oc-link-display")).toHaveValue("Go there");
    await expect(page.locator("#oc-link-tip")).toHaveValue("a tip");
    const remove = page.locator("#oc-modal .oc-btn", { hasText: "Remove Link" });
    await remove.click();
    await expect(modal(page)).toBeHidden();
    expect(await linkAt(page, 1, 1)).toBeNull();
    expect(await cellText(page, 1, 1), "Remove Link keeps the text").toBe("Go there");
  });

  test("Escape cancels without a change", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-address").fill("https://example.com/");
    await page.keyboard.press("Escape");
    await expect(modal(page)).toBeHidden();
    expect(await linkAt(page, 1, 1)).toBeNull();
  });

  test("one undo takes back the link and the text it wrote", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-address").fill("https://example.com/");
    await page.keyboard.press("Enter");
    expect(await cellText(page, 1, 1)).toBe("https://example.com/");
    await ed(page, () => window.opencalcEditor.wasmApi().session_undo());
    expect(await linkAt(page, 1, 1)).toBeNull();
    expect(await cellText(page, 1, 1)).toBe("");
  });

  test("the cell menu: Edit Hyperlink…, Open Hyperlink, Remove Hyperlink", async ({ page }) => {
    await boot(page);
    await link(page, 1, 1, "https://example.com/", "");
    const p = await cellCenter(page, 1, 1);
    await page.mouse.click(p.x, p.y, { button: "right" });
    const menu = page.locator("#sheet-ctx");
    await expect(menu).toBeVisible();
    for (const label of ["Edit Hyperlink…", "Open Hyperlink", "Remove Hyperlink"]) {
      await expect(menu.getByRole("button", { name: label, exact: true })).toBeVisible();
    }
    await menu.getByRole("button", { name: "Open Hyperlink", exact: true }).click();
    expect(await opened(page)).toEqual([["https://example.com/", "_blank", "noopener,noreferrer"]]);

    await page.mouse.click(p.x, p.y, { button: "right" });
    await menu.getByRole("button", { name: "Remove Hyperlink", exact: true }).click();
    expect(await linkAt(page, 1, 1)).toBeNull();
    expect(await cellText(page, 1, 1)).toBe("link");

    // A cell without a link offers to make one.
    await page.mouse.click(p.x, p.y, { button: "right" });
    await expect(menu.getByRole("button", { name: "Link", exact: true })).toBeVisible();
    await expect(menu.getByRole("button", { name: "Open Hyperlink", exact: true })).toHaveCount(0);
  });

  test("xlsx keeps a location link's place, text and tip, and a HYPERLINK formula", async ({ page }) => {
    await boot(page);
    const after = await ed(page, () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_set_cell(0, 1, 1, "Go");
      a.session_set_hyperlink(0, 1, 1, "", "'It''s'!B2", "the tip", "Go");
      a.session_set_cell(0, 2, 2, '=HYPERLINK("#Sheet2!D4","Jump")');
      a.session_set_cell(0, 3, 3, "web");
      a.session_set_hyperlink(0, 3, 3, "https://example.com/a#part", "", "", "");
      const bytes = a.session_save_as("xlsx");
      a.session_new();
      a.session_open(bytes);
      return [1, 2, 3].map((i) => JSON.parse(a.session_hyperlink_at(0, i, i)));
    });
    expect(after[0]).toMatchObject({ location: "'It''s'!B2", tooltip: "the tip", display: "Go", formula: false });
    expect(after[1]).toMatchObject({ location: "Sheet2!D4", formula: true });
    expect(after[2]).toMatchObject({ target: "https://example.com/a#part" });
  });
});

test.describe("Insert Hyperlink from the keyboard (XL-UX-164)", () => {
  test("the arrows move along the rail; Enter on a place is OK", async ({ page }) => {
    await boot(page);
    await openDialog(page, 1, 1);
    await page.locator("#oc-link-to-web").focus();
    await page.keyboard.press("ArrowDown");
    await expect(page.locator("#oc-link-to-place")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#oc-link-to-place")).toBeFocused();
    const leaf = page.locator("#oc-link-tree").getByRole("treeitem", { name: "Sheet2", exact: true });
    await leaf.focus();
    await page.keyboard.press("Enter");
    await expect(modal(page)).toBeHidden();
    expect((await linkAt(page, 1, 1)).location).toBe("Sheet2!A1");
  });

  test("a formula cell's text is not the dialog's to replace", async ({ page }) => {
    await boot(page);
    await ed(page, () => window.opencalcEditor.wasmApi().session_set_cell(0, 1, 1, "=1+2"));
    await openDialog(page, 1, 1);
    await expect(page.locator("#oc-link-display")).toBeDisabled();
    await expect(page.locator("#oc-link-display")).toHaveValue("<<Selection in Document>>");
    await page.locator("#oc-link-address").fill("https://example.com/");
    await page.keyboard.press("Enter");
    expect(await cellText(page, 1, 1)).toBe("=1+2");
    expect((await linkAt(page, 1, 1)).target).toBe("https://example.com/");
  });
});
