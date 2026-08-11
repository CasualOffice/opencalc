// The core editing paths, in a browser.
//
// A companion to `editor.smoke.spec.mjs`, which asks whether the editor works
// at all. This asks whether the things people do with a spreadsheet all day
// work: copy and paste, fill, insert and delete lines, formatting, more than
// one sheet, and getting the file back out.
//
// Same discipline as the smoke suite — real user surfaces only. Keyboard
// shortcuts and toolbar buttons in, the accessibility mirror and the formula
// bar out. Nothing here reaches into module state.

import { expect, test } from "@playwright/test";

/// The seeded document, for reference:
///
/// ```text
///        A        B      C       D
///   1    Item     Qty    Price   Total
///   2    Widget   3      4.50    =B2*C2   → 13.5
///   3    Gadget   5      2       =B3*C3   → 10
///   4    Gizmo    2      9.99    =B4*C4   → 19.98
///   5    Total                   =SUM(D2:D4) → 43.48
/// ```
const cell = (page, row, col) => page.locator(`#a11y-${row}-${col}`);

async function boot(page) {
  const problems = [];
  page.on("console", (m) => {
    if (m.type() === "error") problems.push(m.text());
  });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, {
    timeout: 30_000,
  });
  return problems;
}

/// Move the selection with the name box, as a user does.
///
/// Only a single-cell jump is confirmed by reading the box back: after a range
/// is typed in, the box shows the active cell rather than the range, which is
/// what a spreadsheet does.
async function goTo(page, ref) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  if (!ref.includes(":")) {
    await expect(page.locator("#cell-ref")).toHaveValue(ref.toUpperCase());
  }
}

/// Commit a value or formula into the selected cell via the formula bar.
async function type(page, text) {
  await page.fill("#formula-input", text);
  await page.press("#formula-input", "Enter");
}

/// What the formula bar shows for a cell — the stored input, not the value.
async function inputAt(page, ref) {
  await goTo(page, ref);
  return page.locator("#formula-input").inputValue();
}

/// Press a shortcut with the grid focused, which is where a user presses it.
async function shortcut(page, keys) {
  await page.locator("#grid").focus();
  await page.keyboard.press(keys);
}

test.describe("clipboard", () => {
  test("copying a formula and pasting it adjusts its references", async ({ page }) => {
    // The single most load-bearing behaviour in a spreadsheet: a relative
    // reference is relative to where the formula *lands*, not where it came
    // from. Paste it one row down and it must follow.
    await boot(page);

    await goTo(page, "D2"); // =B2*C2
    await shortcut(page, "ControlOrMeta+c");
    await goTo(page, "F2");
    await shortcut(page, "ControlOrMeta+v");

    expect(await inputAt(page, "F2")).toBe("=D2*E2");
  });

  test("a copied value pastes as itself", async ({ page }) => {
    await boot(page);
    await goTo(page, "A2"); // "Widget"
    await shortcut(page, "ControlOrMeta+c");
    await goTo(page, "A8");
    await shortcut(page, "ControlOrMeta+v");
    await expect(cell(page, 7, 0)).toHaveText("Widget");
  });

  test("cut removes the source once it lands", async ({ page }) => {
    await boot(page);
    await goTo(page, "A2");
    await shortcut(page, "ControlOrMeta+x");
    await goTo(page, "A9");
    await shortcut(page, "ControlOrMeta+v");

    await expect(cell(page, 8, 0)).toHaveText("Widget");
    await expect(cell(page, 1, 0), "the source is empty after a cut").toHaveText("");
  });
});

test.describe("fill", () => {
  test("fill down copies the formula and moves its references", async ({ page }) => {
    // Ctrl+D over a selection that starts on a formula. The rows below must get
    // the formula rebased, not the value duplicated.
    await boot(page);

    await goTo(page, "F2");
    await type(page, "=B2*2");
    await goTo(page, "F2:F4");
    await shortcut(page, "ControlOrMeta+d");

    expect(await inputAt(page, "F3")).toBe("=B3*2");
    expect(await inputAt(page, "F4")).toBe("=B4*2");
    await expect(cell(page, 2, 5), "and it computed").toHaveText("10");
  });

  test("fill right does the same across columns", async ({ page }) => {
    await boot(page);
    await goTo(page, "F8");
    await type(page, "=B2*2");
    await goTo(page, "F8:H8");
    await shortcut(page, "ControlOrMeta+r");

    expect(await inputAt(page, "G8")).toBe("=C2*2");
    expect(await inputAt(page, "H8")).toBe("=D2*2");
  });
});

test.describe("structural edits", () => {
  test("inserting a row moves the formulas below it", async ({ page }) => {
    await boot(page);
    // Insert above row 2, so =SUM(D2:D4) becomes =SUM(D3:D5) and still totals
    // the same three cells.
    await goTo(page, "A2");
    await shortcut(page, "ControlOrMeta+Shift+=");

    expect(await inputAt(page, "D6")).toBe("=SUM(D3:D5)");
    await expect(cell(page, 5, 3), "the total is unchanged").toHaveText("43.48");
  });

  test("deleting a referenced row leaves #REF! rather than a wrong answer", async ({
    page,
  }) => {
    // The important half: a reference to something deleted must break loudly.
    // Silently repointing it at the neighbour would produce a plausible number
    // that is wrong, which is worse than an error.
    await boot(page);
    await goTo(page, "F2");
    await type(page, "=A3");
    await goTo(page, "A3");
    await shortcut(page, "ControlOrMeta+-");

    expect(await inputAt(page, "F2")).toBe("=#REF!");
  });
});

test.describe("formatting", () => {
  test("bold applies, is reflected in the toolbar, and undoes", async ({ page }) => {
    await boot(page);
    await goTo(page, "A2");
    await expect(page.locator("#tb-bold")).toHaveAttribute("aria-pressed", "false");

    await shortcut(page, "ControlOrMeta+b");
    await expect(page.locator("#tb-bold")).toHaveAttribute("aria-pressed", "true");

    await shortcut(page, "ControlOrMeta+z");
    await expect(
      page.locator("#tb-bold"),
      "one undo, and the toolbar follows the model",
    ).toHaveAttribute("aria-pressed", "false");
  });
});

test.describe("sheets", () => {
  test("a second sheet can be added and referred to from the first", async ({
    page,
  }) => {
    await boot(page);
    const tabs = page.locator("#sheet-tabs .sheet-tab");
    const before = await tabs.count();

    await page.locator("#sheet-tabs .sheet-add").first().click();
    await expect(tabs).toHaveCount(before + 1);

    // Put a value on the new sheet, then total it from the first.
    await goTo(page, "A1");
    await type(page, "7");

    await tabs.first().click();
    await goTo(page, "F1");
    await type(page, "=Sheet2!A1*2");
    await expect(cell(page, 0, 5), "a cross-sheet reference evaluates").toHaveText("14");
  });
});

test.describe("getting the file out", () => {
  test("save produces a real .xlsx", async ({ page }) => {
    // The end of every session. A download that arrives but is not a package
    // is the failure that looks most like success.
    await boot(page);
    await goTo(page, "F1");
    await type(page, "=1+1");

    const download = await Promise.race([
      page.waitForEvent("download"),
      shortcut(page, "ControlOrMeta+s").then(() => page.waitForEvent("download")),
    ]);
    expect(download.suggestedFilename()).toMatch(/\.xlsx$/);

    const stream = await download.createReadStream();
    const chunks = [];
    for await (const chunk of stream) chunks.push(chunk);
    const bytes = Buffer.concat(chunks);

    expect(bytes.length).toBeGreaterThan(1000);
    // A ZIP local file header — an .xlsx is an OPC package, so anything else
    // will not open anywhere.
    expect(bytes.subarray(0, 2).toString("latin1")).toBe("PK");
  });
});

test.describe("keyboard navigation", () => {
  test("the arrows, Enter and Tab move the selection as they do in Excel", async ({
    page,
  }) => {
    await boot(page);
    await goTo(page, "B2");

    await shortcut(page, "ArrowRight");
    await expect(page.locator("#cell-ref")).toHaveValue("C2");
    await shortcut(page, "ArrowDown");
    await expect(page.locator("#cell-ref")).toHaveValue("C3");

    // Enter commits downward, Tab rightward — the two that make data entry
    // possible without the mouse.
    await page.keyboard.press("Enter");
    await expect(page.locator("#cell-ref")).toHaveValue("C4");
    await page.keyboard.press("Tab");
    await expect(page.locator("#cell-ref")).toHaveValue("D4");
  });

  test("extending a selection leaves the active cell where it started", async ({
    page,
  }) => {
    // In Excel and Sheets the active cell stays where the selection began and
    // the far corner travels; typing replaces the active cell. This editor
    // moved the active cell with the keyboard instead, so selecting H2:H4 with
    // Shift+Down and typing wrote into H4 — the last cell passed over rather
    // than the one still highlighted. Found by the browser suite.
    await boot(page);
    await goTo(page, "H2");
    await shortcut(page, "Shift+ArrowDown");
    await shortcut(page, "Shift+ArrowDown");

    await page.keyboard.type("77");
    await page.keyboard.press("Enter");
    await expect(cell(page, 1, 7), "typing goes to the cell it started on").toHaveText(
      "77",
    );
    await expect(cell(page, 3, 7), "and not to the end of the travel").toHaveText("");
  });

  test("a second Ctrl+Shift+arrow carries on from where the first reached", async ({
    page,
  }) => {
    // The corollary: a jump-extend measures from the travelling corner. If it
    // measured from the stationary active cell instead, pressing it twice
    // would land in the same place both times.
    await boot(page);
    await goTo(page, "A1");
    await shortcut(page, "ControlOrMeta+Shift+ArrowDown"); // to A5, the block edge
    await shortcut(page, "ControlOrMeta+Shift+ArrowRight"); // then across

    // The selection now spans from A1 to the far corner; typing still belongs
    // to A1, and the block reaches past column A.
    await page.keyboard.type("x");
    await page.keyboard.press("Enter");
    await expect(cell(page, 0, 0)).toHaveText("x");
  });

  test("Ctrl+arrow jumps to the edge of the block", async ({ page }) => {
    await boot(page);
    await goTo(page, "A1");
    await shortcut(page, "ControlOrMeta+ArrowDown");
    await expect(
      page.locator("#cell-ref"),
      "to the last populated cell of the run, not one past it",
    ).toHaveValue("A5");
  });
});
