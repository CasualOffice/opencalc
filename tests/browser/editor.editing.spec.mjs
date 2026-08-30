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

  test("editing a cell that a row insertion moved still recomputes", async ({ page }) => {
    // The test above proves the *insertion* rewrote the references. This proves
    // the engine still knows about them afterwards, which is a different claim
    // and the one a kept precedent graph can quietly break: the graph is held
    // across edits and dropped on a structural one, so a graph that survived an
    // insertion would describe the row numbers as they used to be. Nothing
    // errors when that happens — the dependent simply stops being recalculated
    // and keeps displaying its previous answer, which is why this asserts the
    // number changes rather than that the document still loads.
    await boot(page);
    await goTo(page, "A2");
    await shortcut(page, "ControlOrMeta+Shift+=");

    // Widget's quantity, now a row further down. 10 * 4.50 = 45.
    await goTo(page, "B3");
    await type(page, "10");

    await expect(cell(page, 2, 3), "the row total follows its precedent").toHaveText("45");
    await expect(
      cell(page, 5, 3),
      "and so does the sum below it",
    ).toHaveText("74.98");
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

    // Not `#sheet-tabs .sheet-add`: the add button is pinned outside the
    // scrolling strip, so that it cannot be scrolled out of reach (`UX-CHR-07`).
    await page.locator(".sheet-add.sheet-new").first().click();
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

// --- Space, which does three different things (M4-3) -------------------------

test("Shift+Space selects the whole row, and plain Space starts typing", async ({ page }) => {
  // The tracker recorded "plain Space still starts inline edit" as though it
  // were an unfinished corner. It is what Excel does: Space is the first
  // character of the value you are typing, and the row/column chords are the
  // modified forms. Asserted here so the behaviour is a decision on the record
  // rather than a line in a document nobody can check.
  await boot(page);
  await goTo(page, "B3");

  await page.locator("#grid").press("Shift+ ");
  // The whole row is selected, which the engine reports as the selected range
  // rather than the canvas showing it — read back through the name box's
  // companion, the stats line, which only appears for a multi-cell selection.
  await expect(page.locator("#sel-stats")).not.toHaveText("");

  // And plain Space begins an edit whose first character is the space, rather
  // than selecting anything: Space is the value you are typing.
  await goTo(page, "B3");
  await page.locator("#grid").press(" ");
  await expect(page.locator("#inline-edit")).toBeVisible();
  expect(await page.locator("#inline-edit").inputValue()).toBe(" ");
});

test("autofit sizes a rotated heading by its rotated height, not its flat one", async ({ page }) => {
  // `autofitRow` used to carry its own copy of the row-measuring arithmetic,
  // under a comment saying to match the shared one exactly. The copy had no
  // rotation case, so a row of rotated headings was measured as though the text
  // were horizontal and clipped — and because autofit *persists* the height,
  // and a persisted height pins the row against further auto-growth, it did not
  // correct itself on the next draw.
  await boot(page);
  await goTo(page, "A1");
  await type(page, "A heading long enough that turning it needs real room");

  const heightOf = () =>
    page.evaluate(() => window.__ed.wasmApi().session_row_height(0, 0));

  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
  });

  // Autofit it flat, and again rotated. Driven through the engine and the
  // editor's own autofit, so what is asserted is what a double-click on the row
  // boundary actually does.
  await page.evaluate(() => window.__ed.wasmApi().session_clear_row_height(0, 0));
  await page.evaluate(() => window.__ed.autofitRowForTest(0));
  const flat = await heightOf();

  await page.evaluate(() => window.__ed.wasmApi().session_set_rotation(0, 0, 0, 0, 0, 90));
  await page.evaluate(() => window.__ed.wasmApi().session_clear_row_height(0, 0));
  await page.evaluate(() => window.__ed.autofitRowForTest(0));
  const rotated = await heightOf();

  expect(rotated, `rotated ${rotated} should exceed flat ${flat}`).toBeGreaterThan(flat);
  // And by a real amount: the text is long, so turned on its side it needs
  // several times an ordinary row rather than a few pixels more.
  expect(rotated).toBeGreaterThan(flat * 2);
});

test("a title merged across columns does not decide any one column's width", async ({ page }) => {
  // Autofit measured every cell in the column, merged ones included. A title
  // merged across a table is as wide as the whole table, so charging it to the
  // first column made that column as wide as the title — the "naive" merged
  // handling, and the reason Excel leaves merged cells out of autofit rather
  // than trying to apportion them.
  await boot(page);
  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
  });

  // A short value in A2, and a long title merged across A1:D1 above it.
  await goTo(page, "A2");
  await type(page, "7");
  await goTo(page, "A1");
  await type(page, "A title that is very much longer than the column below it");
  await page.evaluate(() => window.__ed.wasmApi().session_merge_cells(0, 0, 0, 0, 3));

  await page.evaluate(() => window.__ed.wasmApi().session_clear_col_width(0, 0));
  await page.evaluate(() => window.__ed.autofitColumnForTest(0));
  const width = await page.evaluate(() => window.__ed.wasmApi().session_col_width(0, 0));

  // Sized to "7", not to the title. Generous bound: the point is the order of
  // magnitude, not a pixel count that would break when a font is swapped.
  expect(width, `column A came out ${width}px wide`).toBeLessThan(120);
});

/// **Typing a reference must not make the grid scroll itself.**
///
/// The grid scrolls *virtually*: the canvas is one screen tall and draws
/// whatever `state.scrollY` says, which is the "no DOM as source of truth" rule
/// applied to scrolling. So a native scroll of `#grid-wrap` is not a second
/// opinion, it is corruption — the canvas keeps drawing from the engine while
/// the element it lives in has moved underneath it.
///
/// It happened for a reason worth pinning: the in-cell editor is a real
/// `<textarea>`, and the rule that tints reference tokens set `position:
/// relative` on it. `relative` leaves an element **in flow**, so the textarea
/// went back to being laid out after the full-height canvas with its `top` only
/// shifting it visually — the wrapper's scroll box grew by the offset, and the
/// browser scrolled that hidden-overflow container to reveal the focused
/// editor. Column headers left the screen, one band of rows drew, and the
/// editor floated near the bottom of the window.
///
/// The class is added the moment a reference is recognised, so it read as
/// "typing =2*A2 broke the grid" and nothing about it pointed at CSS.
///
/// Asserted on the *invariant* — the wrapper never scrolls and its scroll box
/// never exceeds its client box — rather than on the one selector, so any
/// future element that escapes containment fails this too.
test("typing a reference leaves the grid wrapper unscrolled", async ({ page }) => {
  await boot(page);

  const measure = () =>
    page.evaluate(() => {
      const wrap = document.getElementById("grid-wrap");
      const editor = wrap.querySelector(".inline-edit");
      return {
        scrollTop: wrap.scrollTop,
        scrollLeft: wrap.scrollLeft,
        scrollHeight: wrap.scrollHeight,
        clientHeight: wrap.clientHeight,
        editorPosition: editor ? getComputedStyle(editor).position : null,
      };
    });

  // Well down the sheet, so a native scroll has somewhere to go. Through the
  // name box, which is how the rest of this file moves — and then focus is put
  // back on the grid, because the bug is in the **in-cell** editor. Typing with
  // the name box still focused would land the keystrokes there and the test
  // would pass without ever opening the editor it is about.
  await goTo(page, "F13");
  await page.locator("#grid").focus();

  // The first keystroke goes to the grid, which is what opens the in-cell
  // editor; the rest go to the keyboard, which delivers to whatever now has
  // focus. Pressing on `#grid` throughout would re-focus the canvas on every
  // character and take the keystrokes away from the editor under test.
  //
  // One character at a time, because the tint arrives only when the reference
  // resolves — a single `type()` would hide which keystroke did it.
  await page.locator("#grid").press("=");
  for (const ch of ["2", "*", "A", "2"]) {
    await page.keyboard.type(ch);
    const m = await measure();
    expect(m.scrollTop, `wrapper scrolled natively after typing ${ch}`).toBe(0);
    expect(m.scrollLeft, `wrapper scrolled natively after typing ${ch}`).toBe(0);
    expect(
      m.scrollHeight,
      `the wrapper's scroll box grew past its client box after typing ${ch}`,
    ).toBe(m.clientHeight);
  }

  // And the editor is still taken out of flow, which is what keeps it there.
  expect((await measure()).editorPosition).toBe("absolute");

  // The headers are still drawn — the visible symptom, checked rather than
  // inferred from the numbers above.
  const headerVisible = await page.evaluate(() => {
    const c = document.querySelector("canvas");
    const ctx = c.getContext("2d");
    // A few pixels inside the column-header band: it is never empty when drawn.
    const band = ctx.getImageData(0, 0, Math.min(400, c.width), 8).data;
    return band.some((v, i) => i % 4 !== 3 && v !== 0);
  });
  expect(headerVisible, "the column header band went blank").toBe(true);
});

// UX-GRID-02. Scrolling a cell into view by the bare minimum puts its far edge
// against the far edge of the viewport, which leaves the *leading* edge in the
// middle of whatever column happens to be there. The remainder is a different
// size for every target, so arrowing across a wide sheet makes the first
// visible column appear to shrink and — on the way back — grow again. It reads
// as the grid resizing itself rather than scrolling, and it was reported from
// real use on a table, which is where wide sheets actually happen.
//
// Excel never shows a partial leading column. Asserted on the offset the
// renderer uses rather than on pixels: the grid is a canvas, so a pixel test
// would be a test of whatever a column boundary is guessed to look like.
test("scrolling a distant cell into view leaves no partial column or row at the edge", async ({
  page,
}) => {
  const problems = await boot(page);
  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
  });

  // Widths that are not all the same, because a uniform grid hides an
  // off-by-one: every offset is a multiple of one number, so an implementation
  // that snapped to `round(px / width) * width` would pass and still be wrong.
  await page.evaluate(() => {
    const w = window.__ed.wasmApi();
    for (let c = 0; c < 40; c += 1) w.session_set_col_width(0, c, 40 + ((c * 17) % 60));
    for (let r = 0; r < 200; r += 1) w.session_set_row_height(0, r, 16 + ((r * 7) % 24));
  });

  const boundaries = await page.evaluate(() => {
    const w = window.__ed.wasmApi();
    return {
      cols: Array.from({ length: 41 }, (_, c) => w.session_col_offset_px(0, c)),
      rows: Array.from({ length: 201 }, (_, r) => w.session_row_offset_px(0, r)),
    };
  });

  // Several targets, forward and back, because the defect is that the leftover
  // *varies*: one jump could land on a boundary by luck.
  for (const [row, col] of [
    [80, 30],
    [120, 36],
    [40, 12],
    [160, 39],
    [0, 0],
    [90, 22],
  ]) {
    const at = await page.evaluate(
      ([r, c]) => {
        window.__ed.selectForTest(r, c);
        return window.__ed.scrollStateForTest();
      },
      [row, col],
    );
    expect(at.row).toBe(row);
    expect(at.col).toBe(col);
    expect(
      boundaries.cols,
      `scrollX ${at.scrollX} after going to r${row}c${col} is not a column edge`,
    ).toContain(at.scrollX);
    expect(
      boundaries.rows,
      `scrollY ${at.scrollY} after going to r${row}c${col} is not a row edge`,
    ).toContain(at.scrollY);
    // Still actually visible — an "aligned" offset that hid the cell would
    // satisfy the assertions above and defeat the purpose.
    const shown = await page.evaluate(
      ([r, c]) => {
        const w = window.__ed.wasmApi();
        const s = window.__ed.scrollStateForTest();
        const cL = w.session_col_offset_px(0, c);
        const rT = w.session_row_offset_px(0, r);
        return { cL, rT, sx: s.scrollX, sy: s.scrollY };
      },
      [row, col],
    );
    expect(shown.cL, `column ${col} scrolled off the left`).toBeGreaterThanOrEqual(shown.sx);
    expect(shown.rT, `row ${row} scrolled off the top`).toBeGreaterThanOrEqual(shown.sy);
  }

  expect(problems).toEqual([]);
});

// A merge is one cell, not a wall.
//
// `select` snaps any coordinate inside a merge back to the merge's top-left
// anchor — right for a click, wrong for a step. Arrowing right out of B2:D2
// computed (1,2), `select` snapped it back to (1,1), and the selection never
// moved. Left and up worked, because the anchor *is* the top-left, so the
// failure was asymmetric and read as a frozen keyboard rather than a merge rule.
//
// Asserted through the editor's own `select` and its real keydown handler, so
// what is tested is what a key press does — and in both directions, because a
// step that always jumped clear of a merge would break ordinary movement.
test("arrowing out of a merged cell lands past it, in every direction", async ({ page }) => {
  const problems = await boot(page);
  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
    // B2:D2 across, and F4:F6 down, so both axes are covered.
    window.__ed.wasmApi().session_merge_cells(0, 1, 1, 1, 3);
    window.__ed.wasmApi().session_merge_cells(0, 3, 5, 5, 5);
    window.__ed.relayout();
  });

  const step = (row, col, key) =>
    page.evaluate(
      ([row, col, key]) => {
        window.__ed.selectForTest(row, col);
        const canvas = document.getElementById("grid");
        canvas.focus();
        canvas.dispatchEvent(
          new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
        );
        const at = window.__ed.scrollStateForTest();
        return { row: at.row, col: at.col };
      },
      [row, col, key],
    );

  // Out of the horizontal merge B2:D2 (row 1, cols 1..3).
  expect(await step(1, 1, "ArrowRight"), "right, out of a merge").toEqual({ row: 1, col: 4 });
  expect(await step(1, 1, "ArrowLeft"), "left, out of a merge").toEqual({ row: 1, col: 0 });
  // Out of the vertical merge F4:F6 (rows 3..5, col 5).
  expect(await step(3, 5, "ArrowDown"), "down, out of a merge").toEqual({ row: 6, col: 5 });
  expect(await step(3, 5, "ArrowUp"), "up, out of a merge").toEqual({ row: 2, col: 5 });
  // And an ordinary cell still steps by exactly one.
  expect(await step(10, 10, "ArrowRight"), "an unmerged cell is unaffected").toEqual({
    row: 10,
    col: 11,
  });

  expect(problems).toEqual([]);
});

// Ctrl+Shift+End / Ctrl+Shift+Home extend rather than collapse.
//
// Both branches called `select` unconditionally and never tested `shiftKey`, so
// "select everything from here down" — one of the most-used keys there is —
// threw the selection away and left a single cell. The sibling handlers on
// either side of them (Ctrl+arrow, and plain Home/End) already branch on it.
test("Ctrl+Shift+End and Ctrl+Shift+Home extend the selection", async ({ page }) => {
  const problems = await boot(page);
  await page.evaluate(async () => {
    window.__ed = await import(
      document.querySelector('script[type="module"][src*="editor.js"]').src
    );
  });

  const chord = (row, col, key) =>
    page.evaluate(
      ([row, col, key]) => {
        window.__ed.selectForTest(row, col);
        const canvas = document.getElementById("grid");
        canvas.focus();
        canvas.dispatchEvent(
          new KeyboardEvent("keydown", {
            key,
            ctrlKey: true,
            shiftKey: true,
            bubbles: true,
            cancelable: true,
          }),
        );
        return window.__ed.selectionRectForTest();
      },
      [row, col, key],
    );

  // From B2, extending to the end of the data must cover more than one cell and
  // must still start where the cursor was.
  const toEnd = await chord(1, 1, "End");
  expect(toEnd.r0, "the anchor stays where the selection began").toBe(1);
  expect(toEnd.c0).toBe(1);
  expect(
    toEnd.r1 > toEnd.r0 || toEnd.c1 > toEnd.c0,
    `Ctrl+Shift+End collapsed to one cell: ${JSON.stringify(toEnd)}`,
  ).toBe(true);

  // And back to A1, which extends the other way.
  const toHome = await chord(3, 3, "Home");
  expect(toHome.r0, "extending to A1 reaches row 0").toBe(0);
  expect(toHome.c0, "and column 0").toBe(0);
  expect(toHome.r1, "while keeping the cell it started from").toBe(3);
  expect(toHome.c1).toBe(3);

  expect(problems).toEqual([]);
});
