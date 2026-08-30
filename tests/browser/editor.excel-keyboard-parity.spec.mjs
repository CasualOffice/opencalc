// Excel keyboard parity — the chords `docs/12` §4.1 measured as dead or rebound.
//
// Its sibling `editor.excel-shortcuts.spec.mjs` exists because an audit claimed
// six chords were broken and every one worked. This file is the other half of
// that lesson: of the eleven chords §4.1 reported, **three already worked**
// (`Ctrl+G`, `F5`, `Ctrl+F3`), one was reported as dead when it was in fact
// bound to something destructive (`Ctrl+Shift+U` underlined the selection), and
// one was correct behaviour misread as a fault (`Ctrl+Alt+V` says "clipboard is
// empty" because the clipboard is empty). So each test below states the
// observable it reads and why that observable means what it is read as saying.
//
// Every probe focuses the grid first. Every assertion is on something a user
// can see — a dialog, the name box, the status bar, the rendered formula bar,
// the cell's own contents — never on a saved flag that can lag the display.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  // Focus first, always. Every chord here is bound on the canvas, and a probe
  // that skips this measures a key going to `<body>`.
  await page.locator("#grid").focus();
}

const input = (page, row, col) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [row, col]);
const format = (page, row, col) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_format(0, r, c), [row, col]);

// --- Ctrl+Shift+; — the current time ---------------------------------------
//
// The clock is the host's, not the machine's: the engine reads none of its own
// and `syncClock` hands it one, so a page with a fixed clock is a workbook with
// a fixed clock, and the assertion below can be an exact string rather than a
// shape. The timezone is pinned for the same reason — the stamp is local time
// (a UTC serial puts TODAY() on the wrong day for half the world's evening),
// so a runner in Sydney would otherwise read a different hour than one in Rome.
test.describe("date and time stamps, against a fixed host clock", () => {
  test.use({ timezoneId: "UTC" });

  test("Ctrl+Shift+; writes the current time", async ({ page }) => {
    await page.clock.setFixedTime(new Date("2026-08-30T14:07:00.000Z"));
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.selectForTest(10, 0));
    await page.keyboard.press("Control+Shift+Semicolon");
    // Shift changes the character the key sends: this arrives as `e.key === ":"`.
    // The handler tested only for ";", so the whole time branch was unreachable.
    await expect.poll(() => input(page, 10, 0)).toBe("14:07");
  });

  test("Ctrl+; still writes today's date from the same clock", async ({ page }) => {
    await page.clock.setFixedTime(new Date("2026-08-30T14:07:00.000Z"));
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.selectForTest(11, 0));
    await page.keyboard.press("Control+Semicolon");
    await expect.poll(() => input(page, 11, 0)).toBe("2026-08-30");
  });
});

// --- Ctrl+Shift+U — expand the formula bar ---------------------------------
test("Ctrl+Shift+U expands the formula bar and does not underline the cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 2));
  const before = await format(page, 2, 2);

  await page.keyboard.press("Control+Shift+u");

  // The visible half: the bar grows and the chevron says so.
  await expect(page.locator(".formula-bar")).toHaveClass(/expanded/);
  await expect(page.locator("#fx-expand")).toHaveAttribute("aria-expanded", "true");
  // The half that was actually wrong: the chord used to fall through to
  // Ctrl+U and underline the selection, invisibly on an empty cell.
  expect(await format(page, 2, 2), "the cell must be untouched").toBe(before);
  expect(await format(page, 2, 2)).not.toContain('"u"');

  // And it toggles back, like the chevron it shares.
  await page.keyboard.press("Control+Shift+u");
  await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
});

test("Ctrl+U on its own still underlines", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(2, 3));
  await page.keyboard.press("Control+u");
  await expect.poll(() => format(page, 2, 3)).toContain('"u"');
  // The guard added for Ctrl+Shift+U must not have cost the plain chord.
  await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
});

// --- Ctrl+Shift+F — Format Cells, not a second Find ------------------------
test("Ctrl+Shift+F opens Format cells rather than the find bar", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
  await page.keyboard.press("Control+Shift+f");
  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#oc-modal-title")).toHaveText("Format cells");
  await expect(page.locator("#find-bar")).toBeHidden();
});

test("Ctrl+F still opens the find bar", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+f");
  await expect(page.locator("#find-bar")).toBeVisible();
  await expect(page.locator("#find-input")).toBeFocused();
});

// --- Ctrl+Shift+O — select the cells carrying notes ------------------------
test("Ctrl+Shift+O selects every cell with a note", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 1, 1, "10");
    a.session_set_cell(0, 5, 2, "20");
    a.session_set_cell(0, 8, 0, "30"); // no note: must not be selected
    a.session_set_comment(0, 1, 1, "check this", "", "");
    a.session_set_comment(0, 5, 2, "and this", "", "");
    window.opencalcEditor.selectForTest(0, 0);
  });

  await page.keyboard.press("Control+Shift+o");

  // The name box names the active cell of the resulting bank — B2, the first
  // commented cell — and the status bar counts what the selection covers.
  await expect(page.locator("#cell-ref")).toHaveValue("B2");
  await expect(page.locator("#sel-stats")).toContainText("Count:");
  await expect(page.locator("#sel-stats")).toContainText("2");
  await expect(page.locator("#tb-status")).toHaveText("2 cells with notes");
});

test("Ctrl+Shift+O says so when there are no notes", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(3, 3));
  await page.keyboard.press("Control+Shift+o");
  await expect(page.locator("#tb-status")).toHaveText("no cells with notes on this sheet");
  // And it must not have moved anything while saying it.
  await expect(page.locator("#cell-ref")).toHaveValue("D4");
});

// --- Alt+Down — the in-column pick list ------------------------------------
test("Alt+Down offers the column's entries instead of moving the selection", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "banana");
    a.session_set_cell(0, 1, 0, "apple");
    a.session_set_cell(0, 2, 0, "banana"); // a duplicate: offered once
    a.session_set_cell(0, 0, 1, "elsewhere"); // another column: not offered
    window.opencalcEditor.selectForTest(3, 0);
  });

  await page.keyboard.press("Alt+ArrowDown");

  const menu = page.locator("#sheet-ctx");
  await expect(menu).toBeVisible();
  // Alphabetical and de-duplicated, as Excel's list is.
  await expect(menu.locator("button")).toHaveText(["apple", "banana"]);
  // The chord used to fall through to plain ArrowDown and step to A5.
  await expect(page.locator("#cell-ref")).toHaveValue("A4");

  await menu.getByRole("button", { name: "banana" }).click();
  await expect.poll(() => input(page, 3, 0)).toBe("banana");
});

test("Alt+Down on a column with nothing to offer says so and stays put", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 6));
  await page.keyboard.press("Alt+ArrowDown");
  await expect(page.locator("#tb-status")).toHaveText("no entries in this column to pick from");
  await expect(page.locator("#cell-ref")).toHaveValue("G5");
});

// --- The chords §4.1 reported dead that were not ---------------------------
//
// Characterization, not a fix: these passed before this change and pass after.
// They are here because the audit said otherwise and nothing asserted it.
test("Ctrl+G and F5 both reach the name box", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+g");
  await expect(page.locator("#cell-ref")).toBeFocused();

  await page.locator("#grid").focus();
  await page.keyboard.press("F5");
  await expect(page.locator("#cell-ref")).toBeFocused();
});

test("Ctrl+F3 opens the name manager", async ({ page }) => {
  await boot(page);
  await page.evaluate(() =>
    window.opencalcEditor.wasmApi().session_define_name("Sales", "Sheet1!$A$1:$A$3"));
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+F3");
  const menu = page.locator("#sheet-ctx");
  await expect(menu).toBeVisible();
  await expect(menu).toContainText("Sales");
});

// --- Help must not advertise a chord that does nothing ---------------------
//
// The acceptance rule this file exists under. `Help ▸ Keyboard shortcuts` named
// `F3` for the name manager and no keystroke ever answered it (`docs/12` §9.6);
// the chord is `Ctrl+F3`. Rather than assert one corrected string, this presses
// every chord the panel advertises and requires each to change something
// visible — so the panel cannot drift back into promising a dead key.

async function openShortcutsPanel(page) {
  await page.evaluate(() => {
    const help = [...document.querySelectorAll(".menu-top")].find((b) => /help/i.test(b.textContent));
    help.click();
  });
  await page.evaluate(() => {
    const item = [...document.querySelectorAll(".popmenu *")]
      .find((b) => b.children.length === 0 && /Keyboard shortcuts/i.test(b.textContent));
    item.click();
  });
  await expect(page.locator("#oc-modal-title")).toHaveText("Keyboard shortcuts");
}

test("the shortcuts panel names Ctrl+F3, not the F3 that never worked", async ({ page }) => {
  await boot(page);
  await openShortcutsPanel(page);
  const kbds = await page.locator("#oc-modal-body kbd").allTextContents();
  expect(kbds).toContain("Ctrl+F3");
  expect(kbds, "bare F3 does nothing and must not be advertised").not.toContain("F3");
});

// Everything the panel advertises, pressed. `label` is the exact text the panel
// prints — written out rather than derived from the chord, because a derivation
// is a second place for the two to disagree and this test exists to stop that.
// `prep` runs on a freshly reset document, so a chord whose effect depends on
// history (undo, paste) has one.
const ADVERTISED = [
  // The file chords (`TAURI-012`). `Ctrl+S` was advertised by nothing and bound
  // all along; `Ctrl+O` and `Ctrl+N` were bound by nothing and are now both.
  //
  // New confirms before discarding, and Open raises a file picker — neither
  // leaves a mark on the sheet, so "does something" is read from the dialog and
  // from the picker being clicked rather than from a cell changing. That is the
  // same accommodation the `Ctrl+V` row below already makes, for the same
  // reason: the question is whether the advertised command is live.
  {
    label: "Ctrl+N",
    chord: "Control+n",
    // The confirmation before discarding is the evidence, and it lands in the
    // snapshot as `#oc-modal-title`.
    prep: (p) => edit(p, 0, 0, "unsaved"),
  },
  {
    // Open raises the operating system's file picker, which leaves **no mark**
    // on the page — so the snapshot alone reports it as a chord that did
    // nothing. The mark is made by the editor's own click arriving at the real
    // `#tb-open` input: the listener is installed here and set by the editor,
    // never by the test, which is the difference between observing the command
    // and staging it.
    label: "Ctrl+O",
    chord: "Control+o",
    deliver: async (p) => {
      await p.evaluate(() => {
        const input = document.querySelector("#tb-open");
        delete input.dataset.ocPressed;
        input.addEventListener("click", (e) => {
          e.preventDefault(); // no picker in a headless run
          input.dataset.ocPressed = "1";
        }, { once: true });
      });
      await p.keyboard.press("Control+o");
    },
  },
  { label: "Ctrl+S", chord: "Control+s" },
  { label: "Ctrl+Z", chord: "Control+z", prep: (p) => edit(p, 0, 0, "changed") },
  { label: "Ctrl+Shift+Z", chord: "Control+Shift+z", prep: async (p) => { await edit(p, 0, 0, "changed"); await press(p, "Control+z"); } },
  { label: "Ctrl+X", chord: "Control+x" },
  { label: "Ctrl+C", chord: "Control+c" },
  // Ctrl+V is the one row this harness cannot deliver as a keystroke, and the
  // reason is worth stating rather than skipping: the editor deliberately does
  // *not* bind Ctrl+V — letting the browser raise its own `paste` event is the
  // only way to read the `text/html` flavour without asking for clipboard-read
  // permission — and a synthesized key press raises no such event. So this
  // dispatches the event the real chord would, exactly as
  // `editor.clipboard.spec.mjs` does. It still answers the question the test
  // asks: the advertised command is live.
  {
    label: "Ctrl+V",
    chord: "Control+v",
    prep: (p) => sel(p, 6, 6),
    deliver: (p) => p.evaluate(() => {
      const data = new DataTransfer();
      data.setData("text/plain", "pasted");
      document.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true }));
    }),
  },
  { label: "Ctrl+B", chord: "Control+b" },
  { label: "Ctrl+I", chord: "Control+i" },
  { label: "Ctrl+U", chord: "Control+u" },
  { label: "Ctrl+F", chord: "Control+f" },
  { label: "Ctrl+H", chord: "Control+h" },
  { label: "Ctrl+A", chord: "Control+a" },
  { label: "Ctrl++", chord: "Control+Shift+Equal" },
  { label: "Ctrl+−", chord: "Control+Minus" },
  { label: "F2", chord: "F2" },
  { label: "Enter", chord: "Enter" },
  { label: "Ctrl+G", chord: "Control+g" },
  { label: "F5", chord: "F5" },
  { label: "Ctrl+;", chord: "Control+Semicolon" },
  { label: "Ctrl+Shift+;", chord: "Control+Shift+Semicolon" },
  { label: "Ctrl+1", chord: "Control+1" },
  { label: "Ctrl+Shift+F", chord: "Control+Shift+f" },
  { label: "Ctrl+Shift+U", chord: "Control+Shift+u" },
  { label: "Ctrl+Shift+O", chord: "Control+Shift+o", prep: (p) => p.evaluate(() => window.opencalcEditor.wasmApi().session_set_comment(0, 2, 2, "note", "", "")) },
  { label: "Alt+Down", chord: "Alt+ArrowDown" },
  { label: "Ctrl+F3", chord: "Control+F3" },
];

const press = (page, chord) => page.keyboard.press(chord);
const sel = (page, r, c) => page.evaluate(([a, b]) => window.opencalcEditor.selectForTest(a, b), [r, c]);
const edit = (page, r, c, v) =>
  page.evaluate(([a, b, t]) => window.opencalcEditor.wasmApi().session_set_cell(0, a, b, t), [r, c, v]);

// Everything a chord could visibly do, in one reading.
const observe = (page) => page.evaluate(() => {
  const vis = (s) => {
    const el = document.querySelector(s);
    return el && !el.hidden && getComputedStyle(el).display !== "none" ? "shown" : "hidden";
  };
  const a = window.opencalcEditor.wasmApi();
  let cells = "";
  for (let r = 0; r < 8; r += 1) for (let c = 0; c < 8; c += 1) {
    const v = a.session_cell_input(0, r, c);
    if (v) cells += `${r},${c}=${v};`;
  }
  return [
    document.activeElement ? document.activeElement.id || document.activeElement.tagName : "",
    vis("#find-bar"), vis("#oc-modal"), vis("#inline-edit"),
    document.querySelector("#oc-modal-title").textContent,
    document.querySelector(".formula-bar").className,
    document.querySelector("#cell-ref").value,
    document.querySelector("#tb-status").textContent,
    // Set by the editor's own click reaching `#tb-open` (`TAURI-012`); empty
    // otherwise. A file picker changes nothing else on the page.
    document.querySelector("#tb-open").dataset.ocPressed ?? "",
    JSON.stringify(window.opencalcEditor.selectionRectForTest()),
    a.session_cell_format(0, 2, 2),
    [...document.querySelectorAll(".popmenu")].filter((e) => !e.hidden).map((e) => e.id).join(","),
    cells,
  ].join(" | ");
});

test("every chord the shortcuts panel advertises does something", async ({ page }) => {
  await boot(page);
  await openShortcutsPanel(page);
  const advertised = (await page.locator("#oc-modal-body kbd").allTextContents())
    .map((s) => s.trim()).filter(Boolean);
  // The table above must keep pace with the panel: a row added to one and not
  // the other is exactly the drift this test exists to catch.
  const covered = new Set(ADVERTISED.map(({ label }) => label));
  for (const adv of advertised) {
    expect(covered, `the panel advertises ${adv}; this test must press it`).toContain(adv);
  }

  const failures = [];
  for (const { chord, prep, deliver } of ADVERTISED) {
    // A fresh document each time, so one chord's effect cannot stand in for
    // the next one's.
    await page.evaluate(() => {
      const a = window.opencalcEditor.wasmApi();
      a.session_new();
      a.session_set_cell(0, 0, 0, "alpha");
      a.session_set_cell(0, 1, 0, "beta");
      a.session_set_cell(0, 2, 2, "7");
      document.querySelector("#oc-modal").hidden = true;
      // `#sheet-ctx` is built on the fly and must go; every other `.popmenu` is
      // permanent chrome that the toolbar refresh reaches into by id, so these
      // are *hidden*, never removed — removing them took `#tb-bold` and friends
      // with them and broke the next `select()`.
      for (const m of document.querySelectorAll("#sheet-ctx")) m.remove();
      for (const m of document.querySelectorAll(".popmenu")) m.hidden = true;
      const bar = document.querySelector(".formula-bar");
      bar.classList.remove("expanded");
      const fb = document.querySelector("#find-bar");
      if (!fb.hidden) document.querySelector("#find-close").click();
      window.opencalcEditor.selectForTest(2, 2);
    });
    await page.locator("#grid").focus();
    if (prep) await prep(page);
    await page.locator("#grid").focus();

    const before = await observe(page);
    if (deliver) await deliver(page); else await page.keyboard.press(chord);
    await page.waitForTimeout(250);
    const after = await observe(page);
    if (before === after) failures.push(chord);
    await page.keyboard.press("Escape");
  }
  expect(failures, "advertised chords that changed nothing observable").toEqual([]);
});
