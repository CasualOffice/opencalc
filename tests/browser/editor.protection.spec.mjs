// Sheet protection is enforced by the binding that writes, not by the predicate
// that knows.
//
// `protection_blocks` was tested; *which bindings consult it* was not. So the
// rule was covered and its coverage was not, and six write paths never asked:
// fill (Ctrl+D, Ctrl+R and the fill handle), replace-all and replace-at, merge,
// shift-cells, and paste — including the clipboard paste behind Ctrl+V. A user
// who protected a sheet and pressed Ctrl+V was told `pasted`, and the locked
// cells were gone.
//
// This runs in the browser rather than in `cargo test` for a specific reason:
// `JsError::new` panics off-wasm, so a native test cannot call a binding that
// refuses. That is the documented reason (`UX-PROT-01`) the existing tests
// assert the predicate instead — and asserting the predicate is exactly what
// let this through, because it tests the rule and not its use. In a real wasm
// build the refusal is an ordinary thrown error, so the bindings can be driven
// the way a host drives them.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_new();
    a.session_set_cell(0, 0, 0, "keep");
    a.session_set_cell(0, 1, 0, "also");
  });
}

/** Call a binding and report whether it refused, without throwing across. */
const attempt = (page, fn, args) =>
  page.evaluate(
    ([fn, args]) => {
      try {
        window.opencalcEditor.wasmApi()[fn](...args);
        return "wrote";
      } catch (e) {
        return `refused: ${String((e && e.message) || e)}`;
      }
    },
    [fn, args],
  );

const inputAt = (page, r, c) =>
  page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);

// Every write path a host can reach, with the arguments a host passes.
const WRITES = [
  ["session_fill", [0, 0, 0, 0, 0, 0, 0, 3, 0], "Ctrl+D / Ctrl+R / the fill handle"],
  ["session_replace_all", [0, "keep", "gone", true], "Find and Replace ▸ Replace All"],
  ["session_replace_at", [0, 0, 0, "keep", "gone", true], "Find and Replace ▸ Replace"],
  ["session_merge_cells", [0, 0, 0, 1, 1], "Merge cells"],
  ["session_merge_cells_discarding", [0, 0, 0, 1, 1], "Merge, discarding the rest"],
  ["session_shift_cells", [0, 0, 0, 0, 0, true, true], "Insert cells, shifting down"],
  ["session_clip_paste_mode", [0, 0, 0, "all"], "Ctrl+V from the app's own clipboard"],
  ["session_paste_tsv", [0, 0, 0, "x\ty\nz\tw"], "Ctrl+V of text from another app"],
  ["session_paste_html", [0, 0, 0, JSON.stringify([{ dr: 0, dc: 0, rs: 1, cs: 1, text: "x" }])], "Ctrl+V from Excel"],
];

for (const [fn, args, what] of WRITES) {
  test(`${what} refuses a protected sheet`, async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true));

    const outcome = await attempt(page, fn, args);
    expect(outcome, `${fn} must refuse`).toMatch(/^refused:/);
    expect(outcome).toContain("protected");

    // The point of all of the above: the cells are still there.
    expect(await inputAt(page, 0, 0), "A1 survived").toBe("keep");
    expect(await inputAt(page, 1, 0), "A2 survived").toBe("also");
  });
}

test("a paste is refused over its whole extent, not just its anchor", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    // A1 unlocked, everything below it locked — the shape a real protected
    // sheet has: input cells you may type in, surrounded by ones you may not.
    a.session_set_sheet_protected(0, true);
    a.session_set_cell_protection(0, 0, 0, 0, 0, "locked", false);
  });

  // Guarding only the anchor let a multi-row paste land on the locked rows
  // beneath it, because A1 alone was writable. Every paste path knows its own
  // extent one line before it used to guard, so there was never a reason.
  const tsv = await attempt(page, "session_paste_tsv", [0, 0, 0, "one\ntwo\nthree"]);
  expect(tsv, "a three-row paste onto a one-cell hole").toMatch(/^refused:/);
  expect(await inputAt(page, 1, 0), "A2 was not overwritten").toBe("also");

  // A paste that fits entirely inside the unlocked cell is still allowed.
  const single = await attempt(page, "session_paste_tsv", [0, 0, 0, "just-one"]);
  expect(single, "a one-cell paste into an unlocked cell").toBe("wrote");
  expect(await inputAt(page, 0, 0)).toBe("just-one");
});

test("unprotecting releases every one of those paths again", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, true));
  expect(await attempt(page, "session_fill", [0, 0, 0, 0, 0, 0, 0, 2, 0])).toMatch(/^refused:/);

  // A guard added in nine places is nine chances to leave one latched on.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_sheet_protected(0, false));
  expect(await attempt(page, "session_fill", [0, 0, 0, 0, 0, 0, 0, 2, 0])).toBe("wrote");
  expect(await inputAt(page, 2, 0), "the fill happened").toBe("keep");
  expect(await attempt(page, "session_replace_all", [0, "keep", "grown", true])).toBe("wrote");
  expect(await inputAt(page, 0, 0)).toBe("grown");
  expect(await attempt(page, "session_merge_cells", [0, 6, 0, 7, 1])).toBe("wrote");
});
