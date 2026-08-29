// `Ctrl+S` writes back to the file that was opened — the host-side half.
//
// `docs/83` §2 is the rule: **a document has one save target; `Ctrl+S` commits
// the document to that target and never creates a second document.** Before
// `SAVE-02`, the keystroke raised a Save As panel every time, so a desktop user
// accumulated `opencalc (1).xlsx`, `opencalc (2).xlsx` beside the file they had
// opened and the file they had opened was never updated.
//
// The shell's half — the atomic write, the changed-file comparison, the
// read-only refusal — is `desktop/src/save.rs` and is tested by `cargo test`
// without a window. This is the half the editor owns: *which* command commits
// to the target, what happens to each answer the shell can give, and the two
// things that must never happen — a save that says it happened when it did not,
// and a keystroke that makes a second file.
//
// The `__opencalcNative` stub is `editor.save-reporting.spec.mjs`'s, extended
// with the two functions `SAVE-02` adds. It records every call, because half of
// what is asserted here is about which bridge function was reached: "Ctrl+S did
// not open a panel" is the whole feature.

import { expect, test } from "@playwright/test";

/**
 * Boot the editor with a desktop shell that answers `saveTarget` from a script.
 *
 * `outcomes` is consumed one call at a time and the last one repeats, so a test
 * can say "refused, then written" without the stub knowing anything about the
 * conflict it is modelling.
 */
async function boot(page, outcomes) {
  await page.addInitScript((outcomes) => {
    window.__nativeCalls = [];
    let next = 0;
    const take = () => outcomes[Math.min(next++, outcomes.length - 1)];
    window.__opencalcNative = {
      save: (bytes, ext, adopt) => {
        window.__nativeCalls.push({ fn: "save", ext, adopt: !!adopt, bytes: bytes.length });
        return Promise.resolve(`figures.${ext}`);
      },
      saveTarget: (bytes, force) => {
        window.__nativeCalls.push({ fn: "saveTarget", force: !!force, bytes: bytes.length });
        return Promise.resolve(take());
      },
      clearSaveTarget: () => {
        window.__nativeCalls.push({ fn: "clearSaveTarget" });
        return Promise.resolve();
      },
      setDocument: () => Promise.resolve(),
      syncCapabilities: () => Promise.resolve(null),
      publishMenu: () => Promise.resolve(),
      open: () => Promise.resolve(null),
    };
  }, outcomes);
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "unsaved work");
  });
  await page.waitForTimeout(250);
}

const dirty = (page) => page.evaluate(() => window.opencalcEditor.isDirty());
const calls = (page) => page.evaluate(() => window.__nativeCalls);

const WRITTEN = { status: "written", name: "figures.xlsx" };
const NO_TARGET = { status: "no-target" };

test("Ctrl+S writes back to the opened file and raises no panel", async ({ page }) => {
  await boot(page, [WRITTEN]);
  expect(await dirty(page), "seeded work is unsaved").toBe(true);

  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await expect(page.locator("#tb-status")).toHaveText(/saved figures\.xlsx/);

  // The whole feature: the keystroke reached the file the window already has,
  // and the Save As panel — `save` — was never opened. A panel here is one
  // `opencalc (1).xlsx` per press.
  const seen = await calls(page);
  expect(seen.map((c) => c.fn)).toEqual(["saveTarget"]);
  expect(seen[0].force, "an ordinary save never forces past the file it finds").toBe(false);
  expect(await dirty(page), "the bytes landed, so the document is saved").toBe(false);
});

test("a document with no target acquires one rather than downloading", async ({ page }) => {
  // `docs/83` §2: "A target is acquired, never guessed." A new workbook that
  // has never been saved gets the platform panel, not a file in Downloads.
  await boot(page, [NO_TARGET]);
  const downloads = [];
  page.on("download", (d) => downloads.push(d.suggestedFilename()));

  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await expect(page.locator("#tb-status")).toHaveText(/saved figures\.xlsx/);

  const seen = await calls(page);
  expect(seen.map((c) => c.fn)).toEqual(["saveTarget", "save"]);
  // `adopt`: the file the user names in that panel is where the document lives
  // from now on, so the *next* Ctrl+S goes straight to it. Without this the
  // panel would come back every time and the row would be half done.
  expect(seen[1].adopt, "the acquired file becomes the save target").toBe(true);
  expect(downloads, "acquiring a target is not a download").toEqual([]);
  expect(await dirty(page)).toBe(false);
});

test("a file that changed on disk is not overwritten without being asked", async ({ page }) => {
  // §5.3–5.4. Another window, another application, a sync client: all three
  // present identically, and the second saver is the one who is told.
  const changed = {
    status: "refused",
    kind: "changed",
    name: "figures.xlsx",
    why: "figures.xlsx changed on disk since it was opened",
  };
  await boot(page, [changed, changed, WRITTEN]);

  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await expect(page.locator("#oc-modal-title")).toHaveText(/figures\.xlsx changed on disk/);

  // Cancel: nothing is written and the document stays dirty. A refusal that
  // cleared the bullet would be `SAVE-01` again by another route.
  await page.locator("#oc-modal-body .oc-btn", { hasText: "Cancel" }).click();
  await expect.poll(() => dirty(page)).toBe(true);
  await expect(page.locator("#tb-status")).toContainText(/not saved/);
  expect((await calls(page)).map((c) => c.fn)).toEqual(["saveTarget"]);

  // Asked again, the user overwrites — and only then does the force flag go.
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await page.locator("#oc-modal-body .oc-btn", { hasText: "Overwrite" }).click();
  await expect(page.locator("#tb-status")).toHaveText(/saved figures\.xlsx/);
  const seen = await calls(page);
  expect(seen.map((c) => `${c.fn}:${c.force}`)).toEqual([
    "saveTarget:false",
    "saveTarget:false",
    "saveTarget:true",
  ]);
  expect(await dirty(page)).toBe(false);
});

test("a refusal that is not a question is named, and leaves the document dirty", async ({ page }) => {
  await boot(page, [
    {
      status: "refused",
      kind: "read-only",
      name: "locked.xlsx",
      why: "locked.xlsx is read-only, so it was not written",
    },
  ]);
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await expect(page.locator("#tb-status")).toContainText(/could not save: locked\.xlsx is read-only/);
  expect(await dirty(page), "nothing was written").toBe(true);
  // No panel and no dialog: a read-only file is a report, not a decision the
  // user can make from here.
  expect((await calls(page)).map((c) => c.fn)).toEqual(["saveTarget"]);
  await expect(page.locator("#oc-modal")).toBeHidden();
});

test("File ▸ Download writes a copy and leaves the save target alone", async ({ page }) => {
  // "Downloading is not saving." The menu entry keeps doing what the keystroke
  // used to do, which is the other half of the rule.
  await boot(page, [WRITTEN]);
  await page.evaluate(() => window.opencalcEditor.saveAs("native", "download"));
  await expect(page.locator("#tb-status")).toContainText(/wrote a copy/);

  const seen = await calls(page);
  expect(seen.map((c) => c.fn), "a download never touches the target").toEqual(["save"]);
  expect(seen[0].adopt, "a copy is not where the document lives now").toBe(false);
});

test("a new document does not inherit the last one's file", async ({ page }) => {
  // `docs/83` §3.2: "Missing that clear is how a new document overwrites the
  // last one, and it is the acceptance test in §8."
  await boot(page, [WRITTEN]);
  // The document has a name, the way an opened one does.
  await page.evaluate(() => {
    const bytes = new TextEncoder().encode("a,b\n1,2\n");
    window.opencalcEditor.openBytes(bytes, "figures.csv");
  });
  expect(await page.evaluate(() => window.opencalcEditor.documentName())).toBe("figures.csv");

  await page.evaluate(async () => {
    const sheets = await import("/editor.sheets.js");
    sheets.newDocument();
  });

  expect(
    await page.evaluate(() => window.opencalcEditor.documentName()),
    "a new workbook is not the file that was open a moment ago",
  ).toBe(null);
  expect((await calls(page)).map((c) => c.fn)).toContain("clearSaveTarget");
});

test("the command id the desktop shell clears the save target on still exists", async ({ page }) => {
  // Two definitions of one fact, and this is the gate that stops them drifting.
  // `desktop/src/save.rs` holds `NEW_DOCUMENT_COMMAND = "file.new"` and the
  // operating-system menu handler drops the save target when it sees that id go
  // past — which is what stands between a new workbook and the last document's
  // file until the editor's own `File ▸ New` calls `newDocument()`.
  //
  // The editor derives the id from the English label, so renaming the menu
  // entry renames the id and silently disarms the clear. This is that rename
  // turning something red.
  await boot(page, [WRITTEN]);
  const ids = await page.evaluate(() => window.opencalcEditor.listCommands());
  expect(ids).toContain("file.new");
});

test("a save the shell could not even attempt does not mark the document saved", async ({ page }) => {
  // The bridge itself throwing — the boot window where the shell refuses
  // everything until capabilities arrive is the real shape of this.
  await boot(page, [WRITTEN]);
  await page.evaluate(() => {
    window.__opencalcNative.saveTarget = () => Promise.reject(new Error("canSaveAs is off"));
  });
  await page.locator("#grid").focus();
  await page.keyboard.press("Control+s");
  await expect(page.locator("#tb-status")).toContainText(/could not save: .*canSaveAs is off/);
  expect(await dirty(page)).toBe(true);
});
