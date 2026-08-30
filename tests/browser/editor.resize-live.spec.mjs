// Resizing a column has to reflow its contents *during* the drag.
//
// Reported from a running editor: "expanding width of column and row height ..
// its not realtime .. untill i leave it .. than content rearrange .. not
// fluidly". The client already previewed the *geometry*, so the column edge
// moved — but the display text comes from the engine, and the engine still held
// the old width until the mouse came up. The edge slid out from under
// stationary text and everything snapped at the end.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

/// The engine's own width for a column, in device pixels.
const widthOf = (page, col) =>
  page.evaluate((c) => window.opencalcEditor.wasmApi().session_col_width(0, c), col);

const canUndo = (page) =>
  page.evaluate(() => window.opencalcEditor.wasmApi().session_can_undo());

test("a column reflows while it is being dragged, not when it is released", async ({ page }) => {
  const problems = await boot(page);

  const before = await widthOf(page, 1);
  const undoBefore = await canUndo(page);

  // The column header's right edge, which is where a resize is grabbed.
  const edge = await page.evaluate(() => {
    const ed = window.opencalcEditor;
    return { x: ed.HW + ed.colXAt(1) - ed.HW + ed.colWAt(1), y: ed.HH / 2 };
  }).catch(() => null);

  const box = await page.locator("#grid").boundingBox();
  // Fall back to a measured offset if the internals are not exposed: the point
  // only has to land on the boundary between column B and C.
  const px = edge ? edge.x : 64 * 2 + 40;
  const py = edge ? edge.y : 10;

  await page.mouse.move(box.x + px, box.y + py);
  await page.mouse.down();
  await page.mouse.move(box.x + px + 120, box.y + py, { steps: 6 });

  // Still holding the button: the engine must already know the new width.
  const during = await widthOf(page, 1);
  expect(during, "the engine sees the drag before it is released").toBeGreaterThan(before);

  // And nothing may have become undoable yet — a drag is not an edit until it
  // is let go, and one transaction per mouse-move would bury the undo stack.
  expect(await canUndo(page), "a drag in progress is not undoable").toBe(undoBefore);

  await page.mouse.up();
  await page.waitForTimeout(200);

  expect(await widthOf(page, 1), "the released width sticks").toBeGreaterThan(before);
  expect(await canUndo(page), "and one drag leaves exactly one thing to undo").toBe(true);

  expect(problems, "resizing logged nothing").toEqual([]);
});

// --- Asking for a size, rather than dragging one (UX-DLG-02) -----------------
//
// The other half of resizing is the menu's "Column width…" / "Row height…",
// and it asked with `window.prompt`. A native prompt cannot be styled, does not
// look like the application, blocks the whole page, is suppressed outright in
// some embeds, and in the Tauri shell renders as a *system* dialog carrying the
// app's URL.
//
// **These tests stub `prompt`/`confirm`/`alert` rather than waiting to see one.**
// A real `prompt()` blocks the automation as well as the page, so "no prompt
// appeared" cannot be observed by watching for a hang — and a prompt is not in
// the DOM either, which is how the UX audit first read this surface as "did not
// open". The stub records the call, so a regression names itself.

/// Boot with the three native dialogs replaced by recorders.
async function bootNoNativeDialogs(page) {
  await page.addInitScript(() => {
    window.__nativeDialogs = [];
    for (const name of ["prompt", "confirm", "alert"]) {
      window[name] = (...args) => {
        window.__nativeDialogs.push({ name, message: String(args[0] ?? "") });
        return name === "confirm" ? true : name === "prompt" ? "" : undefined;
      };
    }
  });
  return boot(page);
}

/// The engine's own height for a row, in device pixels.
const heightOf = (page, row) =>
  page.evaluate((r) => window.opencalcEditor.wasmApi().session_row_height(0, r), row);

const nativeDialogs = (page) => page.evaluate(() => window.__nativeDialogs);
const openModal = (page) => page.locator(".oc-modal:not([hidden])");

/// Open the size dialog over a selection of `n` lines starting at `at`, the way
/// the header menu does: the selection is the scope, the index is only which
/// current size gets offered.
const askForSize = (page, axis, at, n) =>
  page.evaluate(([ax, start, count]) => {
    const ed = window.opencalcEditor;
    if (ax === "col") {
      ed.selectForTest(0, start);
      ed.extendSelectionForTest(0, start + count - 1);
    } else {
      ed.selectForTest(start, 0);
      ed.extendSelectionForTest(start + count - 1, 0);
    }
    ed.sizeDialog(ax, start);
  }, [axis, at, n]);

// CI-025: `reuseExistingServer` means a server another checkout started will
// answer on this port and this whole file will pass green against somebody
// else's bytes. Assert the served module is the one under test before
// believing anything below it.
test("the editor served on this port is the checkout under test", async ({ page }) => {
  await boot(page);
  const src = await page.evaluate(async () => {
    const r = await fetch(new URL("./editor.dialogs.js", location.href), { cache: "no-store" });
    return r.text();
  });
  // A marker only this dialog has. Deliberately *not* "does the source contain
  // `window.prompt(`" — the first version of this assertion was that, and it
  // failed on the comment explaining what the dialog replaced. Whether a native
  // prompt is *called* is a question for the running page, and the tests below
  // ask it there.
  expect(src.includes("oc-size-form"), "served editor.dialogs.js has no size dialog").toBe(true);
});

test("Column width… opens the application's own dialog, not a browser prompt", async ({ page }) => {
  const problems = await bootNoNativeDialogs(page);
  await askForSize(page, "col", 1, 1);

  await expect(openModal(page)).toBeVisible();
  expect(await nativeDialogs(page), "a native dialog was opened").toEqual([]);

  // Focus lands in the field with the current size selected, so typing over it
  // is the whole interaction — as it was with the prompt this replaces.
  const field = await page.evaluate(() => {
    const a = document.activeElement;
    return {
      id: a.id,
      value: a.value,
      selected: a.value.slice(a.selectionStart, a.selectionEnd),
      labelled: !!document.querySelector(`label[for="${a.id}"]`),
    };
  });
  expect(field.id, "focus is not in the size field").toBe("oc-size-px");
  expect(Number(field.value), "the field does not offer the current size").toBeGreaterThan(0);
  expect(field.selected, "the value is not selected for overtyping").toBe(field.value);
  expect(field.labelled, "the size field has no label").toBe(true);
  expect(problems, "opening the size dialog logged nothing").toEqual([]);
});

test("Enter applies the typed width to every column in the selection", async ({ page }) => {
  const problems = await bootNoNativeDialogs(page);
  const outside = Math.round(await widthOf(page, 4));
  await askForSize(page, "col", 1, 3);
  await expect(openModal(page)).toBeVisible();

  await page.locator("#oc-size-px").fill("173");
  await page.keyboard.press("Enter");

  await expect(openModal(page)).toHaveCount(0);
  for (const c of [1, 2, 3]) {
    expect(Math.round(await widthOf(page, c)), `column ${c} did not take the size`).toBe(173);
  }
  // The prompt's scope was the selection and no more; keep it that way.
  expect(Math.round(await widthOf(page, 4)), "it resized past the selection").toBe(outside);
  // And the grid gets the keyboard back, or the next arrow key goes nowhere.
  expect(await page.evaluate(() => document.activeElement.id),
    "focus did not return to the grid").toBe("grid");
  expect(await nativeDialogs(page)).toEqual([]);
  expect(problems, "resizing from the dialog logged nothing").toEqual([]);
});

test("Enter applies the typed height to every row in the selection", async ({ page }) => {
  await bootNoNativeDialogs(page);
  await askForSize(page, "row", 2, 3);
  await expect(openModal(page)).toBeVisible();

  await page.locator("#oc-size-px").fill("44");
  await page.keyboard.press("Enter");

  await expect(openModal(page)).toHaveCount(0);
  for (const r of [2, 3, 4]) {
    expect(Math.round(await heightOf(page, r)), `row ${r} did not take the size`).toBe(44);
  }
  expect(await nativeDialogs(page)).toEqual([]);
});

test("Escape leaves the sizes exactly as they were", async ({ page }) => {
  await bootNoNativeDialogs(page);
  const before = await widthOf(page, 1);
  const undoBefore = await canUndo(page);
  await askForSize(page, "col", 1, 3);
  await expect(openModal(page)).toBeVisible();

  await page.locator("#oc-size-px").fill("400");
  await page.keyboard.press("Escape");

  await expect(openModal(page)).toHaveCount(0);
  // The failure mode of a dialog bolted onto something that used to act on the
  // spot: it asks, and resizes anyway.
  expect(await widthOf(page, 1), "Escape resized the column").toBe(before);
  expect(await canUndo(page), "Escape left something on the undo stack").toBe(undoBefore);
  expect(await page.evaluate(() => document.activeElement.id),
    "focus did not return to the grid").toBe("grid");
});

test("a size that is not a number is refused in the field, keeping what was typed", async ({ page }) => {
  await bootNoNativeDialogs(page);
  const before = await widthOf(page, 1);
  await askForSize(page, "col", 1, 1);
  await expect(openModal(page)).toBeVisible();

  await page.locator("#oc-size-px").fill("wide please");
  await page.locator(".oc-modal:not([hidden]) button", { hasText: /^OK$/ }).click();

  // Refused where it was typed, and still open so the typo can be corrected
  // rather than retyped from the menu.
  await expect(openModal(page)).toBeVisible();
  await expect(page.locator("#oc-size-error")).toBeVisible();
  // The refusal grows the control cell downwards, and a centred label slides
  // half an error message down the dialog with it. The label belongs beside
  // the field in both states.
  const drift = await page.evaluate(() => {
    const field = document.querySelector("#oc-size-px").getBoundingClientRect();
    const label = document.querySelector('label[for="oc-size-px"]').getBoundingClientRect();
    return Math.abs((label.top + label.height / 2) - (field.top + field.height / 2));
  });
  expect(drift, "the label drifted away from its field").toBeLessThan(4);
  expect(await page.locator("#oc-size-px").inputValue(),
    "the refusal cleared the field").toBe("wide please");
  expect(await widthOf(page, 1), "a refused size still resized the column").toBe(before);
  expect(await nativeDialogs(page)).toEqual([]);
});

// The tests above call `sizeDialog` directly, which is how the audit reaches
// it — but nobody uses it that way. This one goes through the header menu a
// right-click opens, so the wiring is under test and not just the function.
test("the header menu's Column width… reaches the dialog", async ({ page }) => {
  await bootNoNativeDialogs(page);
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 1);
    window.opencalcEditor.headerMenu("col", 200, 120);
  });
  await page.locator("#sheet-ctx button", { hasText: /^Column width…$/ }).click();

  await expect(openModal(page)).toBeVisible();
  expect(await nativeDialogs(page), "the menu still opens a native dialog").toEqual([]);
  await expect(page.locator("#oc-modal-title")).toHaveText(/^Resize column B$/);
});

// Enter is the OK button, but only until somebody tabs off it: a dialog that
// commits from a focused Cancel cannot be backed out of from the keyboard at
// all, which is the trap in handling Enter globally for the whole modal.
test("Enter on a focused Cancel cancels, and changes nothing", async ({ page }) => {
  await bootNoNativeDialogs(page);
  const before = await widthOf(page, 1);
  await askForSize(page, "col", 1, 1);
  await expect(openModal(page)).toBeVisible();
  await page.locator("#oc-size-px").fill("400");

  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => (document.activeElement.textContent || "").trim()),
    "Tab from the field does not reach Cancel").toBe("Cancel");
  await page.keyboard.press("Enter");

  await expect(openModal(page)).toHaveCount(0);
  expect(await widthOf(page, 1), "Enter on Cancel resized the column").toBe(before);
});

test("Tab stays inside the dialog, which is what aria-modal promises", async ({ page }) => {
  await bootNoNativeDialogs(page);
  await askForSize(page, "col", 1, 1);
  await expect(openModal(page)).toBeVisible();

  const seen = [];
  for (let i = 0; i < 5; i += 1) {
    await page.keyboard.press("Tab");
    seen.push(await page.evaluate(() => {
      const a = document.activeElement;
      return { inside: !!(a.closest && a.closest(".oc-modal")), tag: a.tagName };
    }));
  }
  expect(seen.every((s) => s.inside), `Tab left the dialog: ${JSON.stringify(seen)}`).toBe(true);
});

test("one undo puts back a multi-column resize, as one undo puts back a drag", async ({ page }) => {
  // `UX-DLG-02`'s worker found this and left it, correctly — the brief said to
  // keep the multi-selection behaviour, and the *prompt* had the same fault.
  // It is still a defect: the dialog looped `session_set_col_width` per column
  // inside one `tryEdit`, and each call is its own `session.edit`, so resizing
  // B:D cost three undo steps. The **drag** path already used
  // `session_set_col_width_range` (`editor.core.js`), so the menu and the drag
  // disagreed about what one Ctrl+Z means — and the person who resized three
  // columns is the one who finds out.
  const problems = await bootNoNativeDialogs(page);
  const before = [];
  for (const c of [1, 2, 3]) before.push(Math.round(await widthOf(page, c)));

  await askForSize(page, "col", 1, 3);
  await expect(openModal(page)).toBeVisible();
  await page.locator("#oc-size-px").fill("173");
  await page.keyboard.press("Enter");
  await expect(openModal(page)).toHaveCount(0);
  for (const c of [1, 2, 3]) {
    expect(Math.round(await widthOf(page, c)), `column ${c} did not take the size`).toBe(173);
  }

  await page.keyboard.press("ControlOrMeta+z");

  for (const [i, c] of [1, 2, 3].entries()) {
    expect(
      Math.round(await widthOf(page, c)),
      `column ${c} survived the undo — a resize of three columns should not cost three undos`,
    ).toBe(before[i]);
  }
  expect(await nativeDialogs(page)).toEqual([]);
  expect(problems, "undoing the resize logged nothing").toEqual([]);
});
