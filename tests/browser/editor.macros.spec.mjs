// The macro engine (`MACRO-01`, docs/99 §2 and §5).
//
// Four claims, each tested against the real editor and engine:
//
// 1. **ONLYOFFICE compatibility.** Every file in `macros/onlyoffice-samples/`
//    mirrors one of ONLYOFFICE's published spreadsheet macro samples, using the
//    same API calls; each runs against a fixture and must produce what the
//    sample's documentation says it does.
// 2. **The sandbox holds.** Network, DOM, `importScripts`, dynamic `import()`
//    and the string-to-code paths are refused by name; a loop that never ends
//    is terminated; and in every case the workbook is exactly as it was —
//    including cells the macro wrote before it was stopped.
// 3. **A run is one undo step.** One Ctrl+Z takes back a hundred writes and a
//    format.
// 4. **Unimplemented means refused.** A method the subset lacks throws
//    `NotSupported: <Class>.<Method>` and changes nothing.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

const SAMPLES = path.join(path.dirname(fileURLToPath(import.meta.url)), "macros", "onlyoffice-samples");

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

/// Call one engine function in the page.
const q = (page, fn, ...args) =>
  page.evaluate(([fn, args]) => window.opencalcEditor.wasmApi()[fn](...args), [fn, args]);
const input = (page, r, c, sheet = 0) => q(page, "session_cell_input", sheet, r, c);
const fmt = async (page, r, c, sheet = 0) => JSON.parse(await q(page, "session_cell_format", sheet, r, c));
const merges = async (page, sheet = 0) => JSON.parse(await q(page, "session_merges", sheet));
const rowPx = async (page, r) => JSON.parse(await q(page, "session_row_px", 0, r, 1))[0];
const colPx = async (page, c) => JSON.parse(await q(page, "session_col_px", 0, c, 1))[0];
const run = (page, source, opts) =>
  page.evaluate(([s, o]) => window.opencalcEditor.runMacroForTest(s, o), [source, opts ?? {}]);

/// Every cell the fixture can reach, as one string, so "unchanged" is a
/// comparison rather than a spot check.
async function workbookState(page) {
  return page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    const names = JSON.parse(a.session_sheet_names());
    const out = [names.join("|")];
    names.forEach((_, s) => {
      for (let r = 0; r < 12; r++) for (let c = 0; c < 6; c++) {
        out.push(`${s}:${r}:${c}=${a.session_cell_input(s, r, c)}${a.session_cell_format(s, r, c)}`);
      }
    });
    return out.join("\n");
  });
}

// ---------------------------------------------------------------------------
// 1. The corpus.
//
// `prep` runs in the page before the macro (it may only use `window`); `check`
// asserts the result the sample documents.

const CORPUS = {
  "write-data-to-worksheet-cell": {
    check: async (page) => expect(await input(page, 3, 2)).toBe("Hello world"),
  },
  "change-cell-value": {
    prep: () => window.opencalcEditor.selectForTest(4, 3),
    check: async (page) => {
      expect(await input(page, 0, 0)).toBe("Hello World");
      expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 0, c0: 0, r1: 0, c1: 0 });
    },
  },
  "make-cell-font-bold": {
    prep: () => window.opencalcEditor.wasmApi().session_set_cell(0, 1, 0, "bold me"),
    check: async (page) => {
      expect((await fmt(page, 1, 0)).b).toBe(1);
      expect((await fmt(page, 0, 0)).b).toBeUndefined();
    },
  },
  "change-cell-background-color": {
    check: async (page) => expect((await fmt(page, 2, 1)).bg).toMatch(/0000FA$/i),
  },
  "change-cell-font-color": {
    check: async (page) => expect((await fmt(page, 3, 1)).fc).toMatch(/FF0000$/i),
  },
  "merge-cell-range": {
    check: async (page) => {
      const m = (await merges(page)).map((x) => [x.r0, x.c0, x.r1, x.c1]).sort();
      expect(m).toEqual([[0, 0, 0, 1], [1, 0, 1, 1], [2, 0, 2, 1]]);
    },
  },
  "unmerge-cell-range": {
    prep: () => window.opencalcEditor.wasmApi().session_merge_cells(0, 4, 2, 9, 3),
    check: async (page) => expect(await merges(page)).toEqual([]),
  },
  "set-column-width": {
    // 25 characters of 7 px plus Excel's 5 px of padding.
    check: async (page) => expect(await q(page, "session_col_width", 0, 1)).toBe(180),
  },
  "convert-formulas-into-values": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_set_cell(0, 0, 0, "2");
      a.session_set_cell(0, 0, 1, "=A1*3");
      a.session_set_cell(0, 1, 0, "text");
    },
    check: async (page) => {
      expect(await input(page, 0, 1)).toBe("6");
      expect(await input(page, 0, 0)).toBe("2");
      expect(await input(page, 1, 0)).toBe("text");
    },
  },
  "find-invalid-emails": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      ["ann", "bob", "cy", "di"].forEach((n, r) => a.session_set_cell(0, r, 0, n));
      ["ann@example.com", "not an email", "cy@example.org", "di@"].forEach((e, r) => a.session_set_cell(0, r, 1, e));
    },
    check: async (page) => {
      expect((await fmt(page, 1, 1)).bg).toMatch(/FFA3A3$/i);
      expect((await fmt(page, 3, 1)).bg).toMatch(/FFA3A3$/i);
      expect((await fmt(page, 0, 1)).bg).toBeUndefined();
      expect((await fmt(page, 2, 1)).bg).toBeUndefined();
    },
  },
  "check-data-mismatch": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      const rows = [["Name", "Qty", "Item"], ["a", "1", "apple"], ["b", "two", "pear"], ["c", "3", "7"]];
      rows.forEach((row, r) => row.forEach((v, c) => a.session_set_cell(0, r, c, v)));
    },
    check: async (page) => {
      expect((await fmt(page, 2, 1)).bg).toMatch(/FFD5BF$/i); // B3 "two" among numbers
      expect((await fmt(page, 3, 2)).bg).toMatch(/FFD5BF$/i); // C4 "7" among words
      expect((await fmt(page, 1, 1)).bg).toBeUndefined();
      expect((await fmt(page, 1, 2)).bg).toBeUndefined();
    },
  },
  "highlight-duplicates": {
    prep: () => {
      const e = window.opencalcEditor;
      ["x", "y", "x", "z", "y"].forEach((v, r) => e.wasmApi().session_set_cell(0, r, 0, v));
      e.selectForTest(0, 0);
      e.extendSelectionForTest(4, 0);
    },
    check: async (page) => {
      expect((await fmt(page, 0, 0)).bg).toMatch(/FFFF00$/i);
      expect((await fmt(page, 2, 0)).bg).toMatch(/FFFF00$/i);
      expect((await fmt(page, 1, 0)).bg).toMatch(/CCCCFF$/i);
      expect((await fmt(page, 4, 0)).bg).toMatch(/CCCCFF$/i);
      expect((await fmt(page, 3, 0)).bg).toMatch(/FFFFFF$/i);
    },
  },
  "wrap-characters-with-dashes": {
    prep: () => {
      const e = window.opencalcEditor;
      e.wasmApi().session_set_cell(0, 0, 0, "ab");
      e.wasmApi().session_set_cell(0, 0, 1, "7");
      e.selectForTest(0, 0);
      e.extendSelectionForTest(1, 1);
    },
    check: async (page) => {
      expect(await input(page, 0, 0)).toBe("---a---b---");
      expect(await input(page, 0, 1)).toBe("---7---");
      expect(await input(page, 1, 0)).toBe("");
    },
  },
  "hide-or-unhide-rows-and-columns": {
    check: async (page) => {
      // Rows 5 and 8, 1-based; columns 2 and 4 of A:P, 1-based — B and D.
      expect(await rowPx(page, 4)).toBe(0);
      expect(await rowPx(page, 7)).toBe(0);
      expect(await rowPx(page, 5)).toBeGreaterThan(0);
      expect(await colPx(page, 1)).toBe(0);
      expect(await colPx(page, 3)).toBe(0);
      expect(await colPx(page, 2)).toBeGreaterThan(0);
    },
  },
  "freeze-panes": {
    check: async (page) => {
      expect(JSON.parse(await q(page, "session_frozen", 0))).toEqual({ rows: 10, cols: 4 });
      expect(await page.evaluate(() => window.opencalcEditor.selectionRectForTest())).toEqual({ r0: 9, c0: 0, r1: 9, c1: 0 });
    },
  },
  "unmerge-all-cells": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_set_cell(0, 0, 0, "top");
      a.session_set_cell(0, 5, 4, "end");
      a.session_merge_cells(0, 0, 0, 1, 1);
      a.session_merge_cells(0, 3, 3, 4, 4);
    },
    check: async (page) => {
      expect(await merges(page)).toEqual([]);
      expect(await input(page, 0, 0)).toBe("top");
    },
  },
  "sum-of-highlighted-cells": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      [1, 2, 3, 4, 5].forEach((n, r) => a.session_set_cell(0, r, 0, String(n)));
      a.session_set_fill(0, 1, 0, 1, 0, "5B9BD5", -1, 0);
      a.session_set_fill(0, 3, 0, 3, 0, "5B9BD5", -1, 0);
      a.session_set_fill(0, 4, 0, 4, 0, "FF0000", -1, 0);
    },
    check: async (page) => {
      expect(await input(page, 16, 0)).toBe("The sum: 6");
      expect((await fmt(page, 0, 1)).bg).toMatch(/5B9BD5$/i);
    },
  },
  "sort-range": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      [["3", "c"], ["1", "b"], ["2", "a"], ["1", "a"]].forEach((row, r) => row.forEach((v, c) => a.session_set_cell(0, r, c, v)));
    },
    check: async (page) => {
      const got = [];
      for (let r = 0; r < 4; r++) got.push([await input(page, r, 0), await input(page, r, 1)]);
      expect(got).toEqual([["1", "a"], ["1", "b"], ["2", "a"], ["3", "c"]]);
    },
  },
  "remove-duplicate-sheet": {
    prep: () => {
      const a = window.opencalcEditor.wasmApi();
      a.session_add_sheet();
      a.session_add_sheet();
      for (const s of [0, 1]) { a.session_set_cell(s, 0, 0, "q"); a.session_set_cell(s, 0, 1, "1"); }
      a.session_set_cell(2, 0, 0, "different");
      window.opencalcEditor.renderTabs();
    },
    check: async (page) => {
      expect(JSON.parse(await q(page, "session_sheet_names"))).toEqual(["Sheet1", "Sheet3"]);
    },
  },
};

test("every file in the ONLYOFFICE corpus has a documented expectation", () => {
  const files = fs.readdirSync(SAMPLES).filter((f) => f.endsWith(".js")).map((f) => f.slice(0, -3)).sort();
  expect(files).toEqual(Object.keys(CORPUS).sort());
});

for (const [name, spec] of Object.entries(CORPUS)) {
  test(`ONLYOFFICE sample "${name}" produces its documented result`, async ({ page }) => {
    await boot(page);
    if (spec.prep) await page.evaluate(spec.prep);
    const source = fs.readFileSync(path.join(SAMPLES, `${name}.js`), "utf8");
    const res = await run(page, source);
    expect(res.error).toBeNull();
    expect(res.ok).toBe(true);
    await spec.check(page);
  });
}

test("a read sees the macro's own write, recalculated by the engine", async ({ page }) => {
  await boot(page);
  const res = await run(page, `
    var s = Api.GetActiveSheet();
    s.GetRange("A1").SetValue(2);
    s.GetRange("B1").SetValue("=A1*2");
    s.GetRange("C1").SetValue("B1 is " + s.GetRange("B1").GetValue());
    s.GetRange("C2").SetValue(s.GetRange("B1").GetFormula());
    s.GetRange("C3").SetValue(s.GetRangeByNumber(0, 1).GetAddress(true, true));
  `);
  expect(res.error).toBeNull();
  expect(await input(page, 0, 2)).toBe("B1 is 4");
  expect(await input(page, 1, 2)).toBe("=A1*2");
  expect(await input(page, 2, 2)).toBe("$B$1");
});

test("a macro reads the browser's language, and nothing else of navigator (SEC-026)", async ({ page }) => {
  // ONLYOFFICE's samples format by locale through `navigator.language`. The
  // sandbox removed all of `navigator`, which took that harmless read with it;
  // a frozen subset gives it back without `sendBeacon` or anything that reaches
  // the network or a device.
  await boot(page);
  const lang = await page.evaluate(() => navigator.language);
  const res = await run(page, `
    var s = Api.GetActiveSheet();
    s.GetRange("A1").SetValue(navigator.language);
    s.GetRange("A2").SetValue(String(Array.isArray(navigator.languages) && navigator.languages.length > 0));
    s.GetRange("A3").SetValue(typeof navigator.sendBeacon);
    s.GetRange("A4").SetValue(typeof navigator.clipboard + "," + typeof navigator.serviceWorker + "," + typeof navigator.locks + "," + typeof navigator.storage);
    s.GetRange("A5").SetValue(String(Object.isFrozen(navigator)));
  `);
  expect(res.error).toBeNull();
  expect(await input(page, 0, 0)).toBe(lang);
  expect(await input(page, 1, 0)).toBe("TRUE");
  expect(await input(page, 2, 0)).toBe("undefined");
  expect(await input(page, 3, 0)).toBe("undefined,undefined,undefined,undefined");
  expect(await input(page, 4, 0)).toBe("TRUE");
});

// ---------------------------------------------------------------------------
// 2. The sandbox.

const REFUSED = {
  // The bare name `fetch` is the page's stub (`MACRO-13`), refused by host
  // policy and tested in `editor.macro-network.spec.mjs`; the global is still
  // a refused door.
  "fetch on the global": `self.fetch("http://127.0.0.1:9/exfiltrate");`,
  "fetch on globalThis": `globalThis.fetch("http://127.0.0.1:9/exfiltrate");`,
  document: `document.title = "pwned";`,
  importScripts: `importScripts("http://127.0.0.1:9/x.js");`,
  XMLHttpRequest: `new XMLHttpRequest();`,
  WebSocket: `new WebSocket("ws://127.0.0.1:9/");`,
  EventSource: `new EventSource("http://127.0.0.1:9/");`,
  "fetch through the prototype chain": `
    for (var p = globalThis; p; p = Object.getPrototypeOf(p)) {
      var d = Object.getOwnPropertyDescriptor(p, "fetch");
      if (d) { var f = d.value || d.get.call(globalThis); f("http://127.0.0.1:9/"); }
    }
    throw new Error("no fetch anywhere");`,
  "the Function constructor": `(function () {}).constructor("return fetch")()("http://127.0.0.1:9/");`,
  eval: `eval("1 + 1");`,
  "dynamic import": `import("http://127.0.0.1:9/x.js");`,
};

for (const [what, call] of Object.entries(REFUSED)) {
  test(`a macro reaching for ${what} is refused by name and changes nothing`, async ({ page }) => {
    await boot(page);
    await q(page, "session_set_cell", 0, 0, 0, "before");
    const before = await workbookState(page);
    const res = await run(page, `Api.GetActiveSheet().GetRange("A1").SetValue("after");\n${call}`);
    expect(res.ok).toBe(false);
    expect(res.error.name).toBe("SandboxViolation");
    expect(res.error.message).toMatch(/^SandboxViolation: /);
    expect(res.operations).toEqual([]);
    expect(await workbookState(page)).toBe(before);
  });
}

test("a macro that never ends is terminated at its time limit and changes nothing", async ({ page }) => {
  await boot(page);
  const before = await workbookState(page);
  const t0 = Date.now();
  const res = await run(page, `Api.GetActiveSheet().GetRange("A1").SetValue("half done");\nwhile (true) {}`, { timeLimitMs: 1000 });
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("TimeLimitExceeded");
  expect(Date.now() - t0).toBeLessThan(20_000);
  expect(await workbookState(page)).toBe(before);
  // The editor is still alive and a following run works.
  const again = await run(page, `Api.GetActiveSheet().GetRange("B2").SetValue("next");`);
  expect(again.ok).toBe(true);
  expect(await input(page, 1, 1)).toBe("next");
});

test("a macro past its operation bound is stopped by name and changes nothing", async ({ page }) => {
  await boot(page);
  const before = await workbookState(page);
  const res = await run(page, `for (var i = 1; i <= 11; i++) Api.GetActiveSheet().GetRange("A" + i).SetValue(i);`, { maxOps: 10 });
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("LimitExceeded");
  expect(await workbookState(page)).toBe(before);
});

test("a macro that throws changes nothing, and the error names its line", async ({ page }) => {
  await boot(page);
  const before = await workbookState(page);
  const res = await run(page, `Api.GetActiveSheet().GetRange("A1").SetValue("x");\n\nthrow new RangeError("mine");`);
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("RangeError");
  expect(res.error.line).toBe(3);
  expect(await workbookState(page)).toBe(before);
});

test("a read-only workbook refuses a macro up front rather than half-running it", async ({ page }) => {
  await boot(page);
  await q(page, "session_set_cell", 0, 0, 0, "kept");
  await q(page, "session_set_read_only", true);
  const before = await workbookState(page);
  const res = await run(page, `Api.GetActiveSheet().GetRange("A1").SetValue("changed");`);
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("ReadOnly");
  expect(await workbookState(page)).toBe(before);
});

// ---------------------------------------------------------------------------
// 3. One undo step.

test("a macro that writes 100 cells and bolds a range is undone by one Ctrl+Z", async ({ page }) => {
  await boot(page);
  expect(await q(page, "session_can_undo")).toBe(false);
  const res = await run(page, `
    var s = Api.GetActiveSheet();
    for (var i = 1; i <= 100; i++) s.GetRange("A" + i).SetValue(i * 10);
    s.GetRange("A1:B100").SetBold(true);
  `);
  expect(res.error).toBeNull();
  expect(res.operations.length).toBe(101);
  expect(await input(page, 0, 0)).toBe("10");
  expect(await input(page, 99, 0)).toBe("1000");
  expect((await fmt(page, 50, 1)).b).toBe(1);

  await page.locator("#grid").focus();
  await page.keyboard.press("ControlOrMeta+z");

  for (const r of [0, 49, 99]) expect(await input(page, r, 0)).toBe("");
  expect((await fmt(page, 0, 0)).b).toBeUndefined();
  expect((await fmt(page, 50, 1)).b).toBeUndefined();
  expect(await q(page, "session_can_undo")).toBe(false);
});

// ---------------------------------------------------------------------------
// 4. NotSupported.

test("an unimplemented API method throws NotSupported and changes nothing", async ({ page }) => {
  await boot(page);
  await q(page, "session_set_cell", 0, 0, 0, "b");
  await q(page, "session_set_cell", 0, 1, 0, "a");
  const before = await workbookState(page);
  const res = await run(page, `
    var s = Api.GetActiveSheet();
    s.GetRange("C1").SetValue("written first");
    s.GetRange("A1:A2").SetSort("A1", "xlAscending");
  `);
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("NotSupported");
  expect(res.error.message).toBe("NotSupported: ApiRange.SetSort");
  expect(await workbookState(page)).toBe(before);

  const custom = await run(page, `Api.AddCustomFunction(function f() { return 1; });`);
  expect(custom.error.message).toBe("NotSupported: Api.AddCustomFunction");
  const sheet = await run(page, `Api.GetActiveSheet().AddChart("A1:B2");`);
  expect(sheet.error.message).toBe("NotSupported: ApiWorksheet.AddChart");
});
