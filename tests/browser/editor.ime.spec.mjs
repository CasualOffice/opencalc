// Typing Japanese, Chinese or Korean straight into a selected cell
// (XL-FEAT-023).
//
// In Excel a CJK user selects a cell and types: the input method's preedit
// appears *in the cell*, the Enter that confirms a conversion only confirms the
// conversion, and a second Enter commits the cell. Here the grid's keyboard
// lived on a `<canvas>`, which is not a text-input client, so the browser never
// handed the keystrokes to the input method at all: the first romaji letter
// arrived as a raw "n" that started an edit with a Latin character in it, and
// the composition — if it started at all — started one keystroke late. There
// was no `isComposing` / `keyCode 229` handling anywhere in the editor, so an
// Enter that confirmed a conversion also committed the cell.
//
// Playwright cannot drive a real OS input method. Chromium's DevTools protocol
// can stand in for one: `Input.imeSetComposition` is what the browser process
// calls when the IME updates its preedit, `Input.insertText` is the commit, and
// a key event with `windowsVirtualKeyCode: 229` is the keystroke an IME
// swallows. What that cannot prove — the OS side of the handshake — is listed
// in the tracker row, not hidden here.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  return problems;
}

async function goTo(page, ref) {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await expect(page.locator("#cell-ref")).toHaveValue(ref);
}

/// What a cell stores, read through the formula bar after walking to it.
async function inputAt(page, ref) {
  await goTo(page, ref);
  return page.locator("#formula-input").inputValue();
}

const editing = (page) => page.evaluate(() => window.opencalcEditor.state.editing);

/// An input method, as Chromium's browser process drives one.
function ime(cdp) {
  return {
    /// A keystroke the IME consumed: keyCode 229, whatever key it was.
    async key(key, code = "") {
      await cdp.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key, code, windowsVirtualKeyCode: 229, nativeVirtualKeyCode: 229 });
      await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: 229, nativeVirtualKeyCode: 229 });
    },
    /// The preedit changes.
    async compose(text) {
      await cdp.send("Input.imeSetComposition", { text, selectionStart: text.length, selectionEnd: text.length });
    },
    /// The IME commits `text` and ends the composition.
    async commit(text) {
      await cdp.send("Input.insertText", { text });
    },
    /// Romaji typed one letter at a time, each consumed by the IME.
    async romaji(letters, preedits) {
      for (let i = 0; i < letters.length; i++) {
        await this.key(letters[i], `Key${letters[i].toUpperCase()}`);
        await this.compose(preedits[i]);
      }
    },
  };
}

test.describe("IME composition (XL-FEAT-023)", () => {
  test("Japanese typed into a selected cell composes in the cell; the conversion Enter does not commit it", async ({ page }) => {
    const problems = await boot(page);
    await goTo(page, "B8");
    await page.locator("#grid").focus();
    // The grid holds the keyboard through a text-input client the IME can
    // compose into, with no edit open yet.
    expect(await page.evaluate(() => window.opencalcEditor.gridHasKeyboard())).toBe(true);
    expect(await editing(page)).toBe(false);
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);

    // n-i-h-o-n, each letter swallowed by the IME and shown as preedit.
    await jp.romaji(["n", "i", "h", "o", "n"], ["n", "に", "にh", "にほ", "にほn"]);
    // The preedit is in the cell, as Excel shows it — not somewhere unseen.
    const box = page.locator("#inline-edit");
    await expect(box).toBeVisible();
    await expect(box).toHaveValue("にほn");
    expect(await editing(page), "composition did not open the in-cell editor").toBe(true);
    // Space converts; the IME rewrites the preedit.
    await jp.key(" ", "Space");
    await jp.compose("日本");
    // Enter confirms the conversion. It is the IME's keystroke, not the cell's.
    await jp.key("Enter", "Enter");
    await jp.commit("日本");
    await expect(box).toHaveValue("日本");
    expect(await editing(page), "the Enter that confirmed the conversion also ended the edit").toBe(true);
    await expect(page.locator("#cell-ref"), "the conversion Enter moved the selection").toHaveValue("B8");

    // The second Enter is the user's, and commits and moves down as Excel does.
    await page.keyboard.press("Enter");
    await expect(page.locator("#cell-ref")).toHaveValue("B9");

    // And the next word goes straight into the next cell, with nothing clicked
    // in between — the way a column of entries is typed.
    await jp.romaji(["a"], ["あ"]);
    await jp.key("Enter", "Enter");
    await jp.commit("あ");
    await page.keyboard.press("Enter");
    await expect(page.locator("#cell-ref")).toHaveValue("B10");

    expect(await inputAt(page, "B8"), "the first romaji letter leaked into the value").toBe("日本");
    expect(await inputAt(page, "B9"), "the composition after a commit went nowhere").toBe("あ");
    expect(problems).toEqual([]);
  });

  test("a protected cell refuses a composition and keeps nothing of it", async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true));
    await goTo(page, "B8");
    await page.locator("#grid").focus();
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);
    await jp.romaji(["n", "i"], ["n", "に"]);
    await jp.key("Enter", "Enter");
    await jp.commit("に");
    expect(await editing(page)).toBe(false);
    await expect(page.locator("#tb-status")).toContainText("protected");
    // Nothing the IME committed waits in the hidden host to start a later edit.
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, false));
    await page.locator("#grid").focus();
    await page.keyboard.type("x");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "B8")).toBe("x");
  });

  test("Escape during composition cancels the composition, not the edit", async ({ page }) => {
    await boot(page);
    await goTo(page, "C8");
    await page.locator("#grid").focus();
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);

    await jp.romaji(["n", "i"], ["n", "に"]);
    expect(await editing(page)).toBe(true);
    // Escape is the IME's while it has a preedit: it discards the preedit.
    await jp.key("Escape", "Escape");
    await jp.compose("");
    expect(await editing(page), "Escape during composition cancelled the cell edit").toBe(true);
    await expect(page.locator("#inline-edit")).toHaveValue("");

    // Compose again in the same, still-open edit, and commit it.
    await jp.romaji(["h", "o", "n"], ["h", "ほ", "ほn"]);
    await jp.compose("本");
    await jp.key("Enter", "Enter");
    await jp.commit("本");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "C8")).toBe("本");

    // And a plain Escape, with no composition, still cancels the edit.
    await goTo(page, "C9");
    await page.locator("#grid").focus();
    await jp.key("k", "KeyK");
    await jp.compose("k");
    await jp.commit("k");
    expect(await editing(page)).toBe(true);
    await page.keyboard.press("Escape");
    expect(await editing(page)).toBe(false);
    expect(await inputAt(page, "C9")).toBe("");
  });

  test("Korean composes syllable by syllable without losing a jamo", async ({ page }) => {
    await boot(page);
    await goTo(page, "D8");
    await page.locator("#grid").focus();
    const cdp = await page.context().newCDPSession(page);
    const ko = ime(cdp);

    // 한: ㅎ ㅏ ㄴ. The next consonant ends the syllable and opens the next one
    // in the same keystroke, which is where a late editor drops a jamo.
    await ko.key("g", "KeyG"); await ko.compose("ㅎ");
    await ko.key("k", "KeyK"); await ko.compose("하");
    await ko.key("s", "KeyS"); await ko.compose("한");
    await ko.key("r", "KeyR"); await ko.commit("한"); await ko.compose("ㄱ");
    await ko.key("m", "KeyM"); await ko.compose("그");
    await ko.key("f", "KeyF"); await ko.compose("글");
    await expect(page.locator("#inline-edit")).toHaveValue("한글");
    // Enter ends the last syllable's composition; the IME has it.
    await ko.key("Enter", "Enter");
    await ko.commit("글");
    expect(await editing(page)).toBe(true);
    await expect(page.locator("#cell-ref")).toHaveValue("D8");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "D8")).toBe("한글");
  });

  test("an already-open in-cell editor takes a composition without doubling or committing", async ({ page }) => {
    await boot(page);
    await goTo(page, "A2"); // "Widget"
    await page.locator("#grid").focus();
    await page.keyboard.press("F2");
    expect(await editing(page)).toBe(true);
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);

    await jp.romaji(["n", "i"], ["n", "に"]);
    await jp.compose("日");
    // Tab and Enter while composing belong to the IME.
    await jp.key("Tab", "Tab");
    await jp.key("Enter", "Enter");
    await jp.commit("日");
    await expect(page.locator("#inline-edit")).toHaveValue("Widget日");
    expect(await editing(page)).toBe(true);
    await expect(page.locator("#cell-ref")).toHaveValue("A2");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "A2")).toBe("Widget日");
  });

  test("the formula bar takes a composition, and its conversion Enter does not commit", async ({ page }) => {
    await boot(page);
    await goTo(page, "E8");
    await page.locator("#formula-input").click();
    const cdp = await page.context().newCDPSession(page);
    const zh = ime(cdp);

    // Pinyin: z-h-o-n-g → 中, picked with Space; then w-e-n confirmed with Enter.
    await zh.romaji(["z", "h", "o", "n", "g"], ["z", "zh", "zho", "zhon", "zhong"]);
    await zh.key(" ", "Space");
    await zh.commit("中");
    await zh.key("w", "KeyW"); await zh.compose("w");
    await zh.key("e", "KeyE"); await zh.compose("we");
    await zh.key("n", "KeyN"); await zh.compose("wen");
    await zh.key("Enter", "Enter");
    await zh.commit("文");
    await expect(page.locator("#formula-input")).toHaveValue("中文");
    expect(await editing(page)).toBe(true);
    await expect(page.locator("#cell-ref")).toHaveValue("E8");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "E8")).toBe("中文");
  });

  test("a sheet switched with Ctrl+PageDown still takes a composition", async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_add_sheet("S2"));
    await page.evaluate(() => window.opencalcEditor.switchSheet(0));
    await page.locator("#grid").focus();
    // Leaving a sheet tidies up an edit that is not open; it must not take the
    // grid's keyboard with it.
    await page.keyboard.press("Control+PageDown");
    await expect.poll(() => page.locator(".sheet-tab.active").innerText()).toBe("Sheet2");
    expect(await page.evaluate(() => window.opencalcEditor.gridHasKeyboard())).toBe(true);
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);
    await jp.romaji(["k", "a"], ["k", "か"]);
    await jp.key("Enter", "Enter");
    await jp.commit("か");
    await page.keyboard.press("Enter");
    expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(1, 0, 0))).toBe("か");
  });

  test("keystrokes the IME owns fire no grid shortcut", async ({ page }) => {
    await boot(page);
    await goTo(page, "B2"); // 3
    await page.locator("#grid").focus();
    const cdp = await page.context().newCDPSession(page);
    const jp = ime(cdp);
    // A swallowed Delete, Backspace and arrow on an idle grid: none is the grid's.
    await jp.key("Delete", "Delete");
    await jp.key("Backspace", "Backspace");
    await jp.key("ArrowDown", "ArrowDown");
    await expect(page.locator("#cell-ref")).toHaveValue("B2");
    expect(await page.locator("#formula-input").inputValue()).toBe("3");
    expect(await editing(page)).toBe(false);

    // And once the IME lets go, the grid is still the grid.
    await page.keyboard.press("ArrowDown");
    await expect(page.locator("#cell-ref")).toHaveValue("B3");
    await page.keyboard.type("7");
    await page.keyboard.press("Enter");
    expect(await inputAt(page, "B3")).toBe("7");
  });
});
