// Measure the editing surface, then write the map from what was measured.
//
// This exists because every prose map in this repository has been caught wrong
// in *both* directions — `docs/47` listed Ctrl+X, Ctrl+Space, F4, Ctrl+D/R and
// Ctrl+; as missing when all of them worked, while `docs/73` recorded five
// keyboard defects of which two had been fixed long before. A map maintained by
// hand drifts, and the drift is invisible: it reads exactly like a map that is
// right.
//
// So nothing here is asserted. Each entry drives the real editor and observes a
// real thing, and the document is generated from the results. A behaviour that
// stops working turns the map red on the next run rather than on the next
// complaint.
//
//   cd tests/browser && node ux-sweep.mjs            # table to stdout
//   cd tests/browser && node ux-sweep.mjs --write    # regenerate the map
//
// It lives here rather than in `tools/` because ESM resolves `@playwright/test`
// from the file's own directory, and this is where Playwright is installed.
//
// It needs the editor served at PORT (default 8123); `tests/browser` starts one.

import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";

const PORT = process.env.PORT || 8123;
// Not `URL`: that shadows the global constructor used just below.
const EDITOR = `http://127.0.0.1:${PORT}/editor.html`;
const MAP = new URL("../../docs/47-UX-AND-FEATURE-MAP.md", import.meta.url).pathname;

/** Each check: drive the editor, then answer one yes/no about what happened.
 *
 * `hit` is how often a working spreadsheet user meets this — daily, weekly,
 * rare. `size` is what fixing it costs — s, m, l. Neither is measurable, so
 * both are judgement and are written here rather than pretending otherwise;
 * what the harness contributes is that the *verdict* is not judgement. The
 * pipeline is the misses ordered by hit against size, which is the only
 * ordering that answers "what should we do next" rather than "what is wrong".
 */
const CHECKS = [];

// How often somebody meets a behaviour, and what fixing it costs. Keyed by name
// rather than passed at each call site, so adding a check never has to remember
// this and a weight can never attach to the wrong row.
const WEIGHT = {
  "drag a column header to reorder": ["daily", "m"],
  "drag a row header to reorder": ["daily", "m"],
  "drag the selection border to move a range": ["daily", "m"],
  "Ctrl+click adds a second range": ["daily", "l"],
  "a banked multi-range is what operations act on": ["daily", "l"],
  "the toolbar shows the active cell's number format": ["daily", "s"],
  "the toolbar shows the active cell's fill colour": ["daily", "s"],
  "a mixed selection does not report as uniform": ["daily", "s"],
  "arrowing past a hidden row skips it": ["daily", "s"],
  "undo moves the view to what it just changed": ["daily", "s"],
  "filling a date increments it": ["daily", "s"],
  "filling 'Item 1' continues the number": ["daily", "s"],
  "the zoom level is visible without opening a menu": ["daily", "s"],
  "the filter dropdown offers sorting, as Excel and Sheets do": ["daily", "s"],
  "the filter checklist orders numbers numerically": ["daily", "s"],
  "hovering a font previews it before committing": ["daily", "m"],
  "typing offers entries already in the column": ["daily", "m"],
  "an Alt+Enter entry undoes in one step": ["weekly", "s"],
  "Ctrl+0 does what the Zoom menu says it does": ["weekly", "s"],
  "Ctrl+Backspace scrolls back to the active cell": ["weekly", "s"],
  "reopening validation shows the rule that is there": ["weekly", "s"],
  "deleting a sheet asks first": ["weekly", "s"],
  "row height and column width are reachable from the menu bar": ["weekly", "s"],
  "a locked cell refuses before the user types, not after": ["weekly", "s"],
  "a currency other than $ can be chosen": ["weekly", "s"],
  "Remove Duplicates lets you choose which columns count": ["weekly", "m"],
  "Replace All honours the all-sheets option the Find used": ["weekly", "m"],
  "a picture can be inserted": ["weekly", "m"],
  "Data ▸ Subtotal groups sorted rows": ["weekly", "m"],
  "a quick-analysis affordance appears on a selection": ["weekly", "m"],
  "Flash Fill derives a column from an example": ["weekly", "l"],
  "shrink-to-fit can be turned on": ["rare", "s"],
  "sparklines exist": ["rare", "l"],
};

const check = (area, name, run) => {
  const [hit, size] = WEIGHT[name] || ["weekly", "m"];
  CHECKS.push({ area, name, run, hit, size });
};

// --- helpers every check can use -------------------------------------------
const seed = (page) =>
  page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    for (let r = 0; r < 12; r += 1) {
      for (let c = 0; c < 6; c += 1) {
        a.session_set_cell(0, r, c, `${String.fromCharCode(65 + c)}${r + 1}`);
      }
    }
    window.opencalcEditor.selectForTest(0, 0);
  });

const cell = (page, r, c) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const sel = (page) => page.evaluate(() => window.opencalcEditor.selectionRectForTest());
const centre = (page, r, c) =>
  page.evaluate(([r, c]) => {
    const ed = window.opencalcEditor;
    return { x: ed.colXAt(c) + ed.colWAt(c) / 2, y: ed.rowYAt(r) + ed.rowHAt(r) / 2 };
  }, [r, c]);

// --- the vocabulary ---------------------------------------------------------
// Settled by the first sweep; kept so a regression shows up here rather than in
// somebody's hands.
// Both Excel and Sheets require the band to be selected before it can be
// dragged — a drag from an *unselected* header is drag-to-extend, which is the
// far commoner gesture. A probe that skips the click therefore exercises the
// extend path and reports a working move as missing, which is how this row
// stayed red after `MOVE-02` shipped.
check("Selection", "drag a column header to reorder", async (page, box, hdr) => {
  const before = await cell(page, 0, 0);
  const a = await centre(page, 0, 0), c = await centre(page, 0, 2);
  await page.mouse.click(box.x + a.x, box.y + hdr.h / 2);
  await page.mouse.move(box.x + a.x, box.y + hdr.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + c.x, box.y + hdr.h / 2, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 0, 0)) !== before;
});

check("Selection", "drag a row header to reorder", async (page, box, hdr) => {
  const before = await cell(page, 0, 0);
  const a = await centre(page, 0, 0), c = await centre(page, 3, 0);
  await page.mouse.click(box.x + hdr.w / 2, box.y + a.y);
  await page.mouse.move(box.x + hdr.w / 2, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + hdr.w / 2, box.y + c.y, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 0, 0)) !== before;
});

check("Selection", "drag the selection border to move a range", async (page, box) => {
  const a = await centre(page, 0, 0), t = await centre(page, 5, 3);
  await page.mouse.move(box.x + a.x - 18, box.y + a.y);
  await page.mouse.down();
  await page.mouse.move(box.x + t.x, box.y + t.y, { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(180);
  return (await cell(page, 5, 3)) === "A1";
});

// **This probe used to report a working feature as missing**, and it was the
// largest entry in the fix pipeline for weeks. It read `selectionRectForTest()`,
// which returns the *active* rectangle — and after a Ctrl+click the active
// rectangle is exactly the cell just clicked, so the check could never be true
// no matter how well the bank worked. A probe that cannot observe the thing it
// asks about reports `MISSING` for a feature that is there, which is the same
// cost as a false pass pointing the other way.
check("Selection", "Ctrl+click adds a second range", async (page, box) => {
  const a = await centre(page, 0, 0), d = await centre(page, 4, 4);
  await page.mouse.click(box.x + a.x, box.y + a.y);
  await page.keyboard.down("Control");
  await page.mouse.click(box.x + d.x, box.y + d.y);
  await page.keyboard.up("Control");
  await page.waitForTimeout(120);
  // The bank is what a second range *is*, so the bank is what to look at.
  return (await page.evaluate(() => window.opencalcEditor.allRanges().length)) > 1;
});

check("Selection", "double-click a column border autofits it", async (page, box, hdr) => {
  const w0 = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  const a = await centre(page, 0, 0);
  await page.mouse.dblclick(box.x + a.x + w0 / 2, box.y + hdr.h / 2);
  await page.waitForTimeout(220);
  return (await page.evaluate(() => window.opencalcEditor.colWAt(0))) !== w0;
});

check("Selection", "drag across column headers selects a span", async (page, box, hdr) => {
  const a = await centre(page, 0, 0), c = await centre(page, 0, 3);
  await page.mouse.move(box.x + a.x, box.y + hdr.h / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + c.x, box.y + hdr.h / 2, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(120);
  return (await sel(page)).c1 >= 3;
});

check("Editing", "drag the fill handle fills", async (page, box) => {
  const a = await centre(page, 0, 0);
  const w = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  const h = await page.evaluate(() => window.opencalcEditor.rowHAt(0));
  await page.mouse.move(box.x + a.x + w / 2 - 2, box.y + a.y + h / 2 - 2);
  await page.mouse.down();
  const t = await centre(page, 4, 0);
  await page.mouse.move(box.x + a.x, box.y + t.y, { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(220);
  return (await cell(page, 4, 0)) !== "";
});

check("Sheets", "double-click a sheet tab renames it", async (page) => {
  await page.locator(".sheet-tab").first().dblclick();
  await page.waitForTimeout(180);
  return page.evaluate(
    () => !!document.querySelector(".sheet-tab input, #sheet-rename, .sheet-tab [contenteditable]"),
  );
});

// --- formatting, and what the toolbar tells you about it --------------------
// Enumerated against Excel/Sheets, then measured. The enumeration's own
// guesses are not recorded anywhere: a guess is the thing this replaces.
const fmt = (page, r, c) =>
  page.evaluate(([r, c]) => JSON.parse(window.opencalcEditor.wasmApi().session_cell_format(0, r, c)), [r, c]);

check("Formatting", "bold applies across a multi-cell selection", async (page) => {
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("toolbar.bold");
  });
  await page.waitForTimeout(150);
  return !!(await fmt(page, 0, 0)).b;
});

check("Formatting", "the toolbar shows the active cell's number format", async (page) => {
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.wasmApi().session_set_number_format(0, 0, 0, 0, 0, "0%");
    window.opencalcEditor.selectForTest(1, 0);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(200);
  // A percent cell that looks identical to a General one means the toolbar is
  // not telling you what you are standing on.
  return page.evaluate(() =>
    ["#tb-percent", "#tb-numfmt", "#tb-currency"].some((id) => {
      const el = document.querySelector(id);
      return el && (el.getAttribute("aria-pressed") === "true" || (el.textContent || "").includes("%"));
    }),
  );
});

check("Formatting", "the toolbar shows the active cell's fill colour", async (page) => {
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.wasmApi().session_set_fill(0, 0, 0, 0, 0, "FF0000");
    window.opencalcEditor.selectForTest(1, 0);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.waitForTimeout(200);
  return page.evaluate(() => {
    const el = document.querySelector("#tb-fillcolor");
    if (!el) return false;
    const shown = (el.style.cssText + " " + (el.getAttribute("style") || "")).toLowerCase();
    return shown.includes("ff0000") || shown.includes("255, 0, 0");
  });
});

check("Formatting", "a mixed selection does not report as uniform", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_font_size(0, 0, 0, 0, 0, 14);
    a.session_set_font_size(0, 0, 1, 0, 1, 9);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.evaluate(() => window.opencalcEditor.extendSelectionForTest?.(0, 1));
  await page.waitForTimeout(200);
  // Reading the top-left cell and showing it as the answer is how somebody
  // applies 14pt to a selection they believed was already 14pt.
  const shown = await page.evaluate(() => document.querySelector("#tb-size")?.value ?? "");
  return shown === "" || shown === "—";
});

check("Formatting", "hovering a font previews it before committing", async (page) => {
  const before = await fmt(page, 0, 0);
  await page.evaluate(() => document.querySelector("#tb-font-caret")?.click());
  await page.waitForTimeout(150);
  const item = page.locator("#font-menu .combo-item").nth(3);
  if (!(await item.count())) return false;
  await item.hover();
  await page.waitForTimeout(200);
  const during = await fmt(page, 0, 0);
  return (during.fn || "") !== (before.fn || "");
});

check("View", "the zoom level is visible without opening a menu", async (page) =>
  page.evaluate(() => {
    const text = (document.querySelector(".bottom-bar")?.textContent || "") +
      (document.querySelector(".statusbar")?.textContent || "");
    return /\d{2,3}\s*%/.test(text) || !!document.querySelector("#zoom-level, .zoom-widget, input[type=range][aria-label*=oom]");
  }),
);

check("Formatting", "row height and column width are reachable from the menu bar", async (page) =>
  page.evaluate(() =>
    window.opencalcEditor
      .listCommands()
      .some((id) => /row-height|column-width|row_height|col-width/i.test(id)),
  ),
);

check("Formatting", "shrink-to-fit can be turned on", async (page) =>
  page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    const hasBinding = Object.keys(api).some((k) => /shrink/i.test(k));
    const hasCommand = window.opencalcEditor.listCommands().some((id) => /shrink/i.test(id));
    return hasBinding || hasCommand;
  }),
);

check("Formatting", "a currency other than $ can be chosen", async (page) =>
  page.evaluate(() =>
    window.opencalcEditor.listCommands().some((id) => /currenc/i.test(id)) &&
    !!document.querySelector("[data-nf*='€'], [data-nf*='£'], [data-currency]"),
  ),
);

// --- claims worth measuring because they are worse than "missing" ----------
// Also a false negative, and for a plainer reason: it inspected `state.ranges`
// without ever *making* a second range, so on a fresh page it read an empty
// bank and returned false. Its comment asserted that "Copy takes the active
// range alone, silently" — measured, that is not what happens; formatting
// applies to every banked range. The claim was never checked.
//
// Asserting the *effect* rather than the state: what matters is that an
// operation reaches both ranges, and a bank nothing acts on is not a feature.
check("Selection", "a banked multi-range is what operations act on", async (page, box) => {
  const a = await centre(page, 0, 0), d = await centre(page, 4, 4);
  await page.mouse.click(box.x + a.x, box.y + a.y);
  await page.keyboard.down("Control");
  await page.mouse.click(box.x + d.x, box.y + d.y);
  await page.keyboard.up("Control");
  await page.waitForTimeout(120);
  if ((await page.evaluate(() => window.opencalcEditor.allRanges().length)) < 2) return false;
  await page.evaluate(() => document.getElementById("tb-bold").click());
  await page.waitForTimeout(200);
  const [first, second] = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [
      JSON.parse(a.session_cell_format(0, 0, 0)).b,
      JSON.parse(a.session_cell_format(0, 4, 4)).b,
    ];
  });
  return !!first && !!second;
});

check("View", "Ctrl+0 does what the Zoom menu says it does", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => window.opencalcEditor.setZoomForTest?.(1.5));
  const w0 = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  await page.keyboard.press("Control+0");
  await page.waitForTimeout(200);
  const zoom = await page.evaluate(() => window.opencalcEditor.scrollStateForTest().zoom);
  const w1 = await page.evaluate(() => window.opencalcEditor.colWAt(0));
  // The Zoom menu advertises Ctrl+0 for 100%. If instead it hid a column, the
  // label is not merely wrong — it fires a destructive verb.
  return Math.abs(zoom - 1) < 0.01 && w1 !== 0 && w0 !== 0;
});

check("Editing", "Ctrl+Backspace scrolls back to the active cell", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 0));
  const before = await cell(page, 1, 0);
  await page.keyboard.press("Control+Backspace");
  await page.waitForTimeout(200);
  // Excel scrolls the view back. If the cell is now empty this is worse than
  // missing: an unbound chord fell through to something that deletes.
  return (await cell(page, 1, 0)) === before;
});

check("Editing", "a CRLF paste does not leave a stray carriage return", async (page) => {
  await page.evaluate(() =>
    window.opencalcEditor.wasmApi().session_paste_tsv(0, 7, 0, "a\tb\r\nc\td"),
  );
  await page.waitForTimeout(150);
  // Every paste out of Excel on Windows arrives this way.
  return (await cell(page, 7, 1)) === "b";
});

check("Editing", "an Alt+Enter entry undoes in one step", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => window.opencalcEditor.selectForTest(7, 0));
  await page.evaluate(() => window.opencalcEditor.commit("one\ntwo", false));
  await page.waitForTimeout(150);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  // commit() writes the value and then toggles wrap — two history entries, so
  // the first undo takes the wrap off and leaves the text.
  return (await cell(page, 7, 0)) === "";
});

check("Editing", "undo moves the view to what it just changed", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(1, 0);
    window.opencalcEditor.commit("changed", false);
    window.opencalcEditor.selectForTest(60, 5);
  });
  await page.waitForTimeout(150);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  const s = await page.evaluate(() => window.opencalcEditor.scrollStateForTest());
  // Undoing something you cannot see is indistinguishable from nothing happening.
  return s.row === 1 && s.col === 0;
});

check("Selection", "arrowing past a hidden row skips it", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_hide_rows(0, 3, 3);
    window.opencalcEditor.selectForTest(2, 0);
  });
  await page.waitForTimeout(150);
  await page.keyboard.press("ArrowDown");
  await page.waitForTimeout(150);
  // Parking the cursor on a zero-height row means the next keystroke edits a
  // cell the user cannot see.
  return (await page.evaluate(() => window.opencalcEditor.scrollStateForTest())).row === 4;
});

check("Editing", "filling a date increments it", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 7, 0, "2024-01-01");
    a.session_fill(0, 7, 0, 7, 0, 7, 0, 10, 0);
  });
  await page.waitForTimeout(200);
  return (await cell(page, 8, 0)) !== (await cell(page, 7, 0));
});

check("Editing", "filling 'Item 1' continues the number", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 7, 0, "Item 1");
    a.session_fill(0, 7, 0, 7, 0, 7, 0, 10, 0);
  });
  await page.waitForTimeout(200);
  return (await cell(page, 8, 0)) === "Item 2";
});

check("Editing", "Flash Fill derives a column from an example", async (page) =>
  page.evaluate(() => window.opencalcEditor.listCommands().some((id) => /flash/i.test(id))),
);

check("Editing", "typing offers entries already in the column", async (page) =>
  page.evaluate(() => {
    // Excel's AutoComplete for text. The only completion here is the function
    // catalogue, which does not read the column.
    const src = String(window.opencalcEditor.showAutocompleteForTest || "");
    return /column|neighbour|values/i.test(src);
  }),
);

check("Formatting", "Format Cells leaves a font colour alone if untouched", async (page) => {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  const before = await fmt(page, 0, 0);
  await page.evaluate(() => window.opencalcEditor.runCommand(window.opencalcEditor.listCommands().find((i) => /format.*cell/i.test(i)) || "format.cells"));
  await page.waitForTimeout(300);
  const apply = page.locator(".oc-modal:not([hidden]) button", { hasText: /^(Apply|OK)$/ });
  if (!(await apply.count())) return false;
  await apply.first().click();
  await page.waitForTimeout(250);
  const after = await fmt(page, 0, 0);
  // Stamping 000000 onto a cell that had no colour is a change the user did not
  // ask for, and it survives into the file.
  return (before.fc || "") === (after.fc || "");
});

check("Editing", "a locked cell refuses before the user types, not after", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true);
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.keyboard.press("F2");
  await page.waitForTimeout(200);
  // Letting somebody type a paragraph and refusing at Enter wastes their work.
  return page.evaluate(() => {
    const el = document.getElementById("inline-edit");
    return !el || getComputedStyle(el).display === "none";
  });
});

check("Data", "Remove Duplicates spares data outside the selection", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    ["x", "x", "y", "y", "z", "z"].forEach((v, r) => a.session_set_cell(0, r, 0, v));
    [1, 2, 3, 4, 5, 6].forEach((v, r) => a.session_set_cell(0, r, 4, String(v)));
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
  for (let i = 0; i < 5; i += 1) await page.keyboard.press("Shift+ArrowDown");
  await page.evaluate(() => window.opencalcEditor.runCommand("data.remove-duplicates"));
  await page.waitForTimeout(300);
  const btn = page.locator(".oc-modal:not([hidden]) button").last();
  if (await btn.count()) { await btn.click(); await page.waitForTimeout(350); }
  const left = await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    return [0, 1, 2, 3, 4, 5].map((r) => a.session_cell_input(0, r, 4)).filter(Boolean);
  });
  // Rows may move. Values may not vanish.
  return ["1", "2", "3", "4", "5", "6"].every((v) => left.includes(v));
});

// --- navigation and the things a person does before they type anything ------
const at = (page) => page.evaluate(() => window.opencalcEditor.scrollStateForTest());

check("Navigation", "Ctrl+Arrow jumps to the edge of the data", async (page) => {
  await page.locator("#grid").focus();
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.keyboard.press("Control+ArrowDown");
  await page.waitForTimeout(150);
  return (await at(page)).row === 11;
});

check("Navigation", "Ctrl+End goes to the last used cell", async (page) => {
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+End");
  await page.waitForTimeout(150);
  const s = await at(page);
  return s.row === 11 && s.col === 5;
});

check("Navigation", "the Name Box accepts a range and selects it", async (page) => {
  await page.fill("#cell-ref", "B2:D4");
  await page.press("#cell-ref", "Enter");
  await page.waitForTimeout(200);
  const r = await sel(page);
  return r.r0 === 1 && r.c0 === 1 && r.r1 === 3 && r.c1 === 3;
});

check("Navigation", "the Name Box defines a name for the selection", async (page) => {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.fill("#cell-ref", "Sales");
  await page.press("#cell-ref", "Enter");
  await page.waitForTimeout(250);
  return page.evaluate(() => {
    try { return JSON.parse(window.opencalcEditor.wasmApi().session_names() || "[]").length > 0; }
    catch { return false; }
  });
});

check("Navigation", "each sheet remembers its own scroll and selection", async (page) => {
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.wasmApi().session_add_sheet();
    ed.selectForTest(9, 2);
  });
  await page.waitForTimeout(150);
  await page.evaluate(() => window.opencalcEditor.switchSheet(1));
  await page.waitForTimeout(150);
  await page.evaluate(() => window.opencalcEditor.switchSheet(0));
  await page.waitForTimeout(200);
  const s = await at(page);
  return s.row === 9 && s.col === 2;
});

// --- data tools -------------------------------------------------------------
check("Data", "the filter dropdown offers sorting, as Excel and Sheets do", async (page) => {
  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.filter");
  });
  await page.waitForTimeout(250);
  await page.evaluate(() => window.opencalcEditor.openColumnFilterForTest?.(1));
  await page.waitForTimeout(250);
  return page.evaluate(() => {
    const menu = document.querySelector("#sheet-ctx");
    return !!menu && /A\s*→\s*Z|sort/i.test(menu.textContent || "");
  });
});

check("Data", "the filter checklist orders numbers numerically", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    ["n", "9", "10", "100", "2"].forEach((v, r) => a.session_set_cell(0, r, 0, v));
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("data.filter");
  });
  await page.waitForTimeout(250);
  await page.evaluate(() => window.opencalcEditor.openColumnFilterForTest?.(0));
  await page.waitForTimeout(250);
  const order = await page.evaluate(() =>
    [...document.querySelectorAll("#sheet-ctx .filter-item")].map((e) => e.textContent.trim()).filter(Boolean),
  );
  const nums = order.filter((t) => /^\d+$/.test(t));
  // A BTreeSet of display text gives 10, 100, 2, 9 — which reads as broken.
  return nums.length >= 3 && nums.every((v, i, a) => i === 0 || Number(a[i - 1]) <= Number(v));
});

check("Data", "Remove Duplicates lets you choose which columns count", async (page) => {
  await page.evaluate(() => window.opencalcEditor.runCommand("data.remove-duplicates"));
  await page.waitForTimeout(300);
  return page.evaluate(() =>
    document.querySelectorAll(".oc-modal:not([hidden]) input[type=checkbox]").length > 0,
  );
});

check("Data", "reopening validation shows the rule that is there", async (page) => {
  await page.evaluate(() => {
    const ed = window.opencalcEditor;
    ed.selectForTest(0, 0);
    ed.wasmApi().session_set_validation(0, 0, 0, 0, 0, "whole", "between", "1", "10", true, "");
    ed.selectForTest(5, 5);
    ed.selectForTest(0, 0);
    ed.runCommand("data.data-validation");
  });
  await page.waitForTimeout(350);
  return page.evaluate(() => {
    const sels = [...document.querySelectorAll("#side-panel-body select")];
    return sels.some((s) => /whole/i.test(s.value));
  });
});

check("Data", "deleting a sheet asks first", async (page) => {
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_add_sheet());
  await page.waitForTimeout(150);
  const before = await page.evaluate(() =>
    JSON.parse(window.opencalcEditor.wasmApi().session_sheet_names()).length,
  );
  const ran = await page.evaluate(() => {
    const id = window.opencalcEditor.listCommands().find((i) => /delete.*sheet|sheet.*delete/i.test(i));
    if (!id) return false;
    try { window.opencalcEditor.runCommand(id); } catch { /* a disabled command still counts as found */ }
    return true;
  });
  await page.waitForTimeout(300);
  const after = await page.evaluate(() =>
    JSON.parse(window.opencalcEditor.wasmApi().session_sheet_names()).length,
  );
  const asked = await page.evaluate(() => !!document.querySelector(".oc-modal:not([hidden])"));
  // Deleting a sheet other formulas point at, with no question asked, is not
  // recoverable by anything a user would think to try.
  //
  // `ran` is required: without it this passes when the command simply is not
  // found, which is the check reporting "safe" for a feature that is absent —
  // a pass for the wrong reason is worse than a fail.
  return ran && (asked || after === before);
});

check("Objects", "a picture can be inserted", async (page) =>
  page.evaluate(() => window.opencalcEditor.listCommands().some((i) => /image|picture/i.test(i))),
);

check("Analysis", "sparklines exist", async (page) =>
  page.evaluate(() => window.opencalcEditor.listCommands().some((i) => /sparkline/i.test(i))),
);

check("Analysis", "Data ▸ Subtotal groups sorted rows", async (page) =>
  page.evaluate(() => window.opencalcEditor.listCommands().some((i) => /subtotal/i.test(i))),
);

check("Analysis", "a quick-analysis affordance appears on a selection", async (page) => {
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.waitForTimeout(200);
  return page.evaluate(
    () => !!document.querySelector("#quick-analysis, .quick-analysis, [data-quick-analysis]"),
  );
});

check("Editing", "Replace All honours the all-sheets option the Find used", async (page) => {
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_add_sheet();
    a.session_set_cell(1, 0, 0, "findme");
    a.session_set_cell(0, 0, 0, "findme");
  });
  await page.waitForTimeout(150);
  // session_replace_all takes one sheet index and no options, so a replace
  // launched from an all-sheets Find silently narrows to the current sheet.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_replace_all(0, "findme", "gone", true));
  await page.waitForTimeout(200);
  return (await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(1, 0, 0))) === "gone";
});

// --- runner -----------------------------------------------------------------
const browser = await chromium.launch();
// `--only <substring>` runs a subset. The full sweep takes minutes, which is
// long enough that verifying one row by mutation — revert, watch it go red,
// restore — stops being something anybody does. A subset run never writes the
// map: a partial result must not be able to overwrite a measured one.
const ONLY = (() => {
  const i = process.argv.indexOf("--only");
  return i === -1 ? null : process.argv[i + 1];
})();
if (ONLY && process.argv.includes("--write")) {
  console.error("--only cannot be combined with --write: a subset would blank every row it did not run");
  process.exit(2);
}
const SELECTED = ONLY ? CHECKS.filter((c) => c.name.includes(ONLY)) : CHECKS;
if (ONLY && SELECTED.length === 0) {
  console.error(`--only ${JSON.stringify(ONLY)} matched none of the ${CHECKS.length} checks`);
  process.exit(2);
}

const results = [];
for (const c of SELECTED) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const page = await ctx.newPage();
  try {
    await page.goto(EDITOR, { waitUntil: "networkidle" });
    await page.waitForFunction(
      () => document.querySelector("#tb-status")?.textContent?.startsWith("engine v"),
      null,
      { timeout: 30_000 },
    );
    await seed(page);
    await page.waitForTimeout(200);
    const box = await page.locator("#grid").boundingBox();
    const s = await page.evaluate(() => window.opencalcEditor.scrollStateForTest());
    const hdr = { w: s.bodyX0, h: s.bodyY0 };
    results.push({ ...c, verdict: (await c.run(page, box, hdr)) ? "works" : "missing" });
  } catch (why) {
    // A check that cannot run is not a pass. Named so it is fixed, not ignored.
    results.push({ ...c, verdict: "error", why: String(why.message).slice(0, 60) });
  }
  await ctx.close();
}
await browser.close();

const pad = (s, n) => String(s).padEnd(n);
for (const r of results) {
  console.log(`${pad(r.verdict.toUpperCase(), 8)} ${pad(r.area, 11)} ${r.name}${r.why ? "  — " + r.why : ""}`);
}
const missing = results.filter((r) => r.verdict !== "works").length;
console.log(`\n${results.length - missing}/${results.length} present`);

if (process.argv.includes("--write")) {
  const byArea = new Map();
  for (const r of results) byArea.set(r.area, [...(byArea.get(r.area) || []), r]);
  const mark = { works: "✅", missing: "❌", error: "⚠️" };
  let out = `<!-- GENERATED by tests/browser/ux-sweep.mjs — do not edit by hand.

Every prose map in this repository has been caught wrong in both directions.
This one is measured: each row was driven against the real editor and observed.
Regenerate with \`cd tests/browser && node ux-sweep.mjs --write\`, against a
served tree (\`python3 webapp/serve.py 8123\`).
-->

# UX and feature map

${results.length - missing} of ${results.length} measured behaviours present.

`;
  for (const [area, rows] of byArea) {
    out += `## ${area}\n\n| | behaviour |\n|---|---|\n`;
    for (const r of rows) out += `| ${mark[r.verdict]} | ${r.name} |\n`;
    out += "\n";
  }
  const RANK = { daily: 3, weekly: 2, rare: 1 };
  const COST = { s: 1, m: 2, l: 4 };
  const todo = results
    .filter((r) => r.verdict !== "works")
    .sort((a, b) => RANK[b.hit] / COST[b.size] - RANK[a.hit] / COST[a.size]);
  out += `## What to fix, in order\n\n`;
  out += `Ranked by how often somebody meets it against what it costs. Those two\n`;
  out += `are judgement and are declared in the harness; the verdict above is not.\n\n`;
  out += `| | behaviour | met | size |\n|---|---|---|---|\n`;
  for (const r of todo) out += `| ${mark[r.verdict]} | ${r.name} | ${r.hit} | ${r.size} |\n`;
  writeFileSync(MAP, out);
  console.log(`\nwrote ${MAP}`);
}
process.exit(0);
