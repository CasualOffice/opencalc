// Custom worksheet functions (`MACRO-04`, docs/99): ONLYOFFICE's
// `Api.AddCustomFunction`, run in the macro sandbox, with Excel's async model.
//
// A function is written in the Macros window's Custom functions section and
// saved; a formula calling it shows `#BUSY!` until the sandboxed worker
// answers, then the answer. The JavaScript never runs on the page or in the
// engine: it runs in a worker with no network and a time limit per call, so a
// function that loops is cut off with `#VALUE!` while the editor carries on.
// The source is part of the workbook (ONLYOFFICE's `xl/jsaProject.bin`), so it
// survives a save and a reopen; a reopened workbook keeps its saved values and
// runs nothing until Enable is chosen, and never where the mode withholds
// `canRunMacros`.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.forgetMacroAnswersForTest?.();
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

const code = (page) => page.locator("#oc-mw-code");
const result = (page) => page.locator("#oc-mw-result");
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
/// What the grid shows in a cell.
const shown = (page, r, c) =>
  page.evaluate(([r, c]) => JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, r, c, r, c))[0]?.t ?? "", [r, c]);

async function writeCode(page, text) {
  await code(page).click();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await page.keyboard.press("Delete");
  await code(page).fill(text);
}

/// Type into a cell the way a person does.
async function typeAt(page, r, c, text) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c), [r, c]);
  await page.locator("#grid").focus();
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

/// Write a custom-function source in the Macros window and save it.
async function defineFunctions(page, source) {
  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new-function").click();
  await writeCode(page, source);
  await page.locator("#oc-mw-run").click();
  await expect(result(page)).toHaveText(/Registered|Line|Error|error/, { timeout: 20_000 });
  const text = await result(page).textContent();
  await page.locator("#oc-mw-close").click();
  return text;
}

const DOUBLE = [
  "/**",
  " * Doubles a number.",
  " * @customfunction",
  " * @param {number} x The number to double.",
  " * @returns {number} Twice x.",
  " */",
  "function DOUBLE(x) { return x * 2; }",
  "Api.AddCustomFunction(DOUBLE);",
].join("\n");

test("a function written in the Macros window computes in a formula", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Alt+F8");
  // Its own section, under its own heading, as ONLYOFFICE's window has it.
  await expect(page.locator("#oc-mw-new-function")).toHaveText("New function");
  await page.locator("#oc-mw-close").click();
  const said = await defineFunctions(page, DOUBLE);
  expect(said).toContain("Registered: DOUBLE");
  await typeAt(page, 0, 0, "=DOUBLE(21)");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("42");
  // A dependent follows.
  await typeAt(page, 0, 1, "=A1+1");
  await expect.poll(() => shown(page, 0, 1)).toBe("43");
  // Changed arguments are asked again.
  await typeAt(page, 1, 0, "5");
  await typeAt(page, 0, 0, "=DOUBLE(A2)");
  await expect.poll(() => shown(page, 0, 0)).toBe("10");
  await typeAt(page, 1, 0, "6");
  await expect.poll(() => shown(page, 0, 0)).toBe("12");
});

test("an async function shows #BUSY! until its promise settles", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, [
    "/**",
    " * Answers later.",
    " * @customfunction",
    " * @param {number} x",
    " * @returns {number}",
    " */",
    "async function SLOW(x) { await new Promise((r) => setTimeout(r, 1500)); return x + 1; }",
    "Api.AddCustomFunction(SLOW);",
  ].join("\n"));
  await typeAt(page, 0, 0, "=SLOW(1)");
  await expect.poll(() => shown(page, 0, 0)).toBe("#BUSY!");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("2");
});

test("this.address is the calling cell and this.args[i].address the argument's", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, [
    "(function () {",
    "  /**",
    "   * Where am I?",
    "   * @customfunction",
    "   * @returns {string} The calling cell.",
    "   */",
    "  function WHERE() { return this.address; }",
    "  Api.AddCustomFunction(WHERE);",
    "  /**",
    "   * Where is my argument?",
    "   * @customfunction",
    "   * @param {any} x",
    "   * @returns {string}",
    "   */",
    "  function WHENCE(x) { return this.args[0].address; }",
    "  Api.AddCustomFunction(WHENCE);",
    "})();",
  ].join("\n"));
  await typeAt(page, 2, 1, "=WHERE()");
  await typeAt(page, 3, 1, "=WHENCE(A1)");
  await expect.poll(() => shown(page, 2, 1), { timeout: 10_000 }).toBe("Sheet1!B3");
  await expect.poll(() => shown(page, 3, 1)).toBe("Sheet1!A1");
});

test("a function that never returns is cut off with #VALUE! and the editor stays responsive", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.setCustomFunctionLimitForTest(500));
  await defineFunctions(page, [
    "/**",
    " * Spins.",
    " * @customfunction",
    " * @returns {number}",
    " */",
    "function SPIN() { for (;;) {} }",
    "Api.AddCustomFunction(SPIN);",
    "/**",
    " * Doubles.",
    " * @customfunction",
    " * @param {number} x",
    " * @returns {number}",
    " */",
    "function TWICE(x) { return 2 * x; }",
    "Api.AddCustomFunction(TWICE);",
  ].join("\n"));
  await typeAt(page, 0, 0, "=SPIN()");
  // While it spins, the editor takes input as ever.
  await typeAt(page, 5, 5, "still here");
  expect(await input(page, 5, 5)).toBe("still here");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("#VALUE!");
  await expect(page.locator("#tb-status")).toContainText("ran longer than 500 ms and was stopped");
  // The sandbox was restarted, and the next call works.
  await typeAt(page, 1, 0, "=TWICE(4)");
  await expect.poll(() => shown(page, 1, 0), { timeout: 10_000 }).toBe("8");
});

test("a function that reaches for the network is refused by the sandbox", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, [
    "/**",
    " * Phones home.",
    " * @customfunction",
    " * @returns {any}",
    " */",
    "async function NET() { const r = await fetch('https://example.com/'); return r.status; }",
    "Api.AddCustomFunction(NET);",
  ].join("\n"));
  await typeAt(page, 0, 0, "=NET()");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("#VALUE!");
  await expect(page.locator("#tb-status")).toContainText("SandboxViolation: fetch is not available");
});

test("a source without its JSDoc is refused as ONLYOFFICE refuses it, by line", async ({ page }) => {
  await boot(page);
  const said = await defineFunctions(page, "function BARE(x) { return x; }\nApi.AddCustomFunction(BARE);");
  expect(said).toContain("Invalid parameters type in JSDOC or options.");
  await typeAt(page, 0, 0, "=BARE(1)");
  await expect.poll(() => shown(page, 0, 0)).toBe("#NAME?");
});

test("Insert Function lists the workbook's functions under User Defined, and autocomplete offers them", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, DOUBLE);
  await page.locator("#grid").focus();
  await page.keyboard.press("Shift+F3");
  await page.locator("#oc-fn-cat").selectOption("User Defined");
  await expect(page.locator("#oc-fn-list .fn-row")).toHaveText(["DOUBLE"]);
  await expect(page.locator("#oc-fn-syntax")).toHaveText("DOUBLE(x)");
  await expect(page.locator("#oc-fn-desc")).toHaveText("Doubles a number.");
  // OK goes on to Function Arguments, with the JSDoc's argument help, as a
  // built-in's does; OK there writes the call into the cell.
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Function Arguments");
  await expect(page.locator("#oc-fa-fdesc")).toHaveText("Doubles a number.");
  await page.locator("#oc-fa-arg-0").focus();
  await expect(page.locator("#oc-fa-adesc")).toHaveText("x: The number to double.");
  await page.locator("#oc-fa-arg-0").fill("21");
  await page.locator("#oc-modal .oc-confirm-actions .oc-btn.primary").click();
  expect(await input(page, 0, 0)).toBe("=DOUBLE(21)");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("42");

  await page.evaluate(() => window.opencalcEditor.selectForTest(4, 4));
  await page.locator("#grid").focus();
  await page.keyboard.type("=DOU");
  await expect(page.locator("#ac-menu")).toContainText("DOUBLE");
  await page.keyboard.press("Escape");
});

/// Save as `.xlsx` and open the bytes again.
async function saveAndReopen(page, name) {
  const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
  await page.evaluate(([b, n]) => window.opencalcEditor.openBytes(new Uint8Array(b), n), [bytes, name]);
  return bytes;
}

test("save and reopen keep the source and the formula; nothing runs until Enable", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, DOUBLE);
  await typeAt(page, 0, 0, "=DOUBLE(21)");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("42");
  const bytes = await saveAndReopen(page, "functions.xlsx");
  expect(Buffer.from(bytes).includes(Buffer.from("xl/jsaProject.bin"))).toBe(true);

  const sources = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_macros()).customFunctions);
  expect(sources.map((f) => f.source)).toEqual([DOUBLE]);
  expect(await input(page, 0, 0)).toBe("=DOUBLE(21)");
  // The saved value, and the offer — not a run.
  await expect(page.locator("#oc-cf-bar")).toBeVisible();
  expect(await shown(page, 0, 0)).toBe("42");
  expect(JSON.parse(await page.evaluate(() => window.opencalcEditor.wasmApi().session_custom_status())).registered).toBe(0);

  await page.locator("#oc-cf-bar-enable").click();
  await expect(page.locator("#oc-cf-bar")).toBeHidden();
  await expect
    .poll(() => page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_custom_status()).registered), { timeout: 10_000 })
    .toBe(1);
  await typeAt(page, 1, 0, "=DOUBLE(4)");
  await expect.poll(() => shown(page, 1, 0), { timeout: 10_000 }).toBe("8");
  await expect.poll(() => shown(page, 0, 0)).toBe("42");
});

for (const [mode, caps] of [
  ["embedded without canRunMacros", { canRunMacros: false }],
  ["read-only", { readOnly: true }],
]) {
  test(`${mode}: the saved values show and no code runs`, async ({ page }) => {
    await boot(page);
    await defineFunctions(page, DOUBLE);
    await typeAt(page, 0, 0, "=DOUBLE(21)");
    await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("42");
    const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
    await page.evaluate(([b, c]) => {
      window.opencalcEditor.setCapabilities(c);
      window.opencalcEditor.openBytes(new Uint8Array(b), "kept.xlsx");
    }, [bytes, caps]);
    expect(await input(page, 0, 0)).toBe("=DOUBLE(21)");
    await expect.poll(() => shown(page, 0, 0)).toBe("42");
    await expect(page.locator("#oc-cf-bar")).toBeHidden();
    // Give a tick every chance to run code it should not.
    await page.evaluate(() => window.opencalcEditor.customFunctionsSettledForTest(1000));
    const st = JSON.parse(await page.evaluate(() => window.opencalcEditor.wasmApi().session_custom_status()));
    expect(st.registered, "nothing was registered, so nothing ran").toBe(0);
    expect(await shown(page, 0, 0)).toBe("42");
  });
}
