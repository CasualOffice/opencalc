// The browser editor opens and writes back what the *engine* supports
// (`WASM-01`).
//
// `SessionFormat` knows which extension is which format, which extension a save
// writes, and which MIME type those bytes are. The bridge hard-coded
// `SessionFormat::Xlsx` and the page kept a second extension table of its own,
// so all of that stopped at the browser boundary: the editor opened exactly the
// formats somebody had last remembered to list in two places, and every
// delimited download was labelled `text/csv` whichever of the three it was.
//
// The shape of every assertion here is the same one: **compare what the page
// does against what the engine says**, never against a literal copied from the
// engine. A test that asserted `".xlsx,.csv,.tsv,.psv"` would pass with the
// second table still in place, which is the defect.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// **The file picker offers what the engine can read.**
///
/// `.tab` is the one that shows the two lists had drifted: `casual-calc-io` has
/// always read it as tab-separated, and the markup's `accept` never mentioned
/// it — so a user with a `.tab` file could not pick it at all.
test("the file picker offers every format the engine can open", async ({ page }) => {
  await boot(page);
  const at = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    return {
      accept: document.getElementById("tb-open").accept.split(",").map((s) => s.trim()),
      engine: JSON.parse(ed.wasmApi().openable_extensions()),
    };
  });

  expect(at.engine, "the engine offered nothing at all").toContain(".tab");
  for (const ext of at.engine) {
    expect(at.accept, `the engine reads ${ext} and the picker does not offer it`).toContain(ext);
  }
});

/// **An extension the engine does not claim is refused, not guessed at.**
///
/// The bytes here are a *genuine package*, so nothing about the file stops it
/// being opened — only the rule that the extension is what names the format.
/// That is deliberate: `SessionFormat::for_extension` answers `None` rather
/// than falling back to `Xlsx` precisely so a file is never opened as one thing
/// and saved back as another under its original name, and the editor guessing
/// where the SDK refuses to puts the guess back.
///
/// A package that *fails* to parse would not test this at all — it would fail
/// the same way whether the extension was consulted or not.
test("a package under an extension the engine does not claim is refused", async ({ page }) => {
  await boot(page);
  const at = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();

    w.session_set_cell(0, 0, 0, "from the package");
    const realPackage = w.session_save();

    w.session_new();
    w.session_set_cell(0, 0, 0, "still here");
    const ok = ed.openBytes(realPackage, "quarterly.xls");
    return {
      ok,
      status: document.getElementById("tb-status").textContent,
      a1: w.session_cell_input(0, 0, 0),
      known: w.format_for_extension("xls"),
    };
  });

  expect(at.known, "the engine claims it can open .xls").toBe("");
  expect(at.ok, "a .xls was opened by guessing it was really a package").toBe(false);
  expect(at.a1, "a refused open replaced the workbook that was loaded").toBe("still here");
  expect(
    at.status,
    `the refusal did not say which format was the problem; it said: ${at.status}`,
  ).toMatch(/\.xls is not a format/);
});

/// **A session opened as TSV downloads as TSV, with the type the engine names.**
///
/// Three things had to be asked of the engine and none were: which format the
/// session came from, what extension that format writes, and what those bytes
/// are. The MIME type is the one that cannot be got right by accident —
/// `text/csv` was written for CSV, TSV and PSV alike.
test("a workbook opened as TSV downloads as TSV, with the engine's content type", async ({ page }) => {
  await boot(page);

  const at = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();

    // Captured rather than performed: the assertion is about the name and the
    // type the page put on the file, and a real download is a dialog nobody can
    // assert on. Both hooks are restored before returning.
    const made = [];
    const names = [];
    const realCreate = URL.createObjectURL;
    const realRevoke = URL.revokeObjectURL;
    const realClick = HTMLAnchorElement.prototype.click;
    URL.createObjectURL = (blob) => { made.push(blob); return "blob:captured"; };
    URL.revokeObjectURL = () => {};
    HTMLAnchorElement.prototype.click = function () { names.push(this.download); };

    let opened = false;
    try {
      const tsv = new TextEncoder().encode("region\tunits\nnorth\t4\nsouth\t7\n");
      opened = ed.openBytes(tsv, "q3.tab");

      // Through the menu the user actually has, by command id, so the test
      // cannot pass against an action no menu reaches.
      const item = document.querySelector('[data-oc-command="file.download.same-format-as-opened"]');
      if (item) {
        item.click();
        // `saveAs` is async; give its microtasks a turn, and answer the loss
        // dialog if the format cannot carry the document.
        for (let i = 0; i < 40 && !names.length; i += 1) {
          await new Promise((r) => setTimeout(r, 25));
          const modal = document.getElementById("oc-modal");
          if (modal && !modal.hidden) document.querySelector("#oc-modal-body .oc-btn.primary")?.click();
        }
      }
      return {
        opened,
        hadMenuItem: !!item,
        name: names[0] ?? null,
        type: made[0] ? made[0].type : null,
        // What the engine says, asked separately so the two can be compared.
        engineExt: w.session_format(),
        engineType: w.session_format_content_type(),
      };
    } finally {
      URL.createObjectURL = realCreate;
      URL.revokeObjectURL = realRevoke;
      HTMLAnchorElement.prototype.click = realClick;
    }
  });

  expect(at.opened, "the editor could not open a .tab file the engine reads").toBe(true);
  // `.tab` is tab-separated: the session's own format is TSV, not the extension
  // it arrived under, and not the package the bridge used to assume.
  expect(at.engineExt, "the session did not remember it was opened as delimited text").toBe("tsv");
  expect(at.hadMenuItem, "there is no way to download the format that was opened").toBe(true);
  expect(at.name, "the download was not named for the format it holds").toBe("opencalc.tsv");
  expect(
    at.type,
    `the download claims to be ${at.type}; the engine says ${at.engineType}`,
  ).toBe(at.engineType);
  // Named explicitly as well, so a regression that made *both* sides wrong in
  // the same way cannot pass the comparison above.
  expect(at.engineType).toBe("text/tab-separated-values;charset=utf-8");
});

/// **And the CSV/TSV/PSV export path takes its type from the engine too.**
///
/// The same defect on the other download: `text/csv;charset=utf-8` was written
/// for all three formats.
test("the delimited export is labelled with the engine's type, not text/csv", async ({ page }) => {
  await boot(page);

  const at = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    const made = [];
    const realCreate = URL.createObjectURL;
    const realRevoke = URL.revokeObjectURL;
    const realClick = HTMLAnchorElement.prototype.click;
    URL.createObjectURL = (blob) => { made.push(blob); return "blob:captured"; };
    URL.revokeObjectURL = () => {};
    HTMLAnchorElement.prototype.click = function () { made.push(this.download); };
    try {
      w.session_new();
      w.session_set_cell(0, 0, 0, "a");
      document.querySelector('[data-oc-command="file.download.tab-separated-tsv"]')?.click();
      for (let i = 0; i < 40 && !made.some((m) => typeof m === "string"); i += 1) {
        await new Promise((r) => setTimeout(r, 25));
      }
      return {
        type: made.find((m) => typeof m !== "string")?.type ?? null,
        engineType: w.format_content_type("tsv"),
      };
    } finally {
      URL.createObjectURL = realCreate;
      URL.revokeObjectURL = realRevoke;
      HTMLAnchorElement.prototype.click = realClick;
    }
  });

  expect(at.engineType).toBe("text/tab-separated-values;charset=utf-8");
  expect(at.type, "a .tsv export went out labelled as something else").toBe(at.engineType);
});
