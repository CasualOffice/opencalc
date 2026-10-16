// Open and save must not freeze the editor (`PERF-16`).
//
// Both used to be one engine call on the thread that draws the grid: ~0.3 s
// to open and ~0.3 s to save a 250,000-cell workbook in this browser, more on
// a slower machine, during which no frame was drawn and no input handled.
// They now run in `file.worker.js`, and what is left here is reading the
// result in parts between frames.
//
// **What is measured.** The longest gap between two animation frames while
// the job runs, through the same routes a person uses (File ▸ Open's file
// input, File ▸ Download). A frame gap is what a person sees as a freeze; the
// acceptance is under 50 ms (docs/30). One workbook size, chosen so the old
// path misses that by several times on a fast machine — not a benchmark, a
// threshold far enough from both sides that a slow runner does not flip it.

import { expect, test } from "@playwright/test";
import fs from "node:fs";

/// 50,000 rows x 5 columns: numbers, strings, a formula column.
const ROWS = 50_000;
/// docs/30: no frame gap of 50 ms or more — on the machine the numbers below
/// were measured on.
const GAP_LIMIT_MS = 50;
/// What the old page-thread path cost for this workbook on that machine, in
/// one call: `session_open_as` 320 ms, `session_save_as("xlsx")` 277 ms.
///
/// **Why the limit is scaled.** A frame gap is wall-clock, and a shared CI
/// runner — or this suite beside six others — is several times slower than
/// the machine the target was set on; an absolute 50 ms is then the flaky
/// wall-clock assertion `PERF-15` removed from the profiler. So each test
/// times the old path's one call on *this* machine first, and the limit is
/// 50 ms times how much slower this machine is (never less than 50). That
/// keeps it a test of the design and not of the runner: the page-thread path
/// freezes for the whole call, ~6x past the scaled limit at any speed, and
/// the worker path leaves only bounded slices.
const REFERENCE_MS = { open: 320, save: 277 };

function gapLimit(kind, pageMs) {
  return Math.round(GAP_LIMIT_MS * Math.max(1, pageMs / REFERENCE_MS[kind]));
}

async function boot(page) {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// Fill the session with the large workbook; the document is left clean, so
/// no "discard changes?" question interrupts an open.
async function fillLarge(page, rows = ROWS) {
  await page.evaluate(async (rows) => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    w.session_new();
    const lines = [];
    for (let i = 0; i < rows; i++) lines.push(`${i}\tname-${i}\t${i * 1.5}\t=A${i + 1}*2\tx${i % 97}`);
    w.session_paste_tsv(0, 0, 0, lines.join("\n"));
    ed.markSaved();
  }, rows);
}

/// Time the page-thread path's one call on this machine: what the old open
/// or save froze the editor for.
async function pageThreadMs(page, kind) {
  return await page.evaluate(async (kind) => {
    const w = (await import(window.__editorModule)).wasmApi();
    const bytes = w.session_save_as("xlsx");
    const t = performance.now();
    if (kind === "save") w.session_save_as("xlsx");
    else w.session_open_as("xlsx", bytes);
    return Math.round(performance.now() - t);
  }, kind);
}

/// The large workbook as an `.xlsx` file on disk, built in the page.
///
/// A path rather than a buffer for `setInputFiles`: Playwright delivers a
/// buffer by decoding it in the page, on the very thread being measured, and
/// that alone is an 80 ms gap. A path is handed to the browser as a file, as
/// a person's pick is.
async function largeXlsx(page) {
  await fillLarge(page);
  const pageMs = await pageThreadMs(page, "open");
  const b64 = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const bytes = ed.wasmApi().session_save_as("xlsx");
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    const w = ed.wasmApi();
    w.session_new();
    w.session_set_cell(0, 0, 0, "untouched");
    ed.markSaved();
    return btoa(s);
  });
  const file = test.info().outputPath("big.xlsx");
  fs.writeFileSync(file, Buffer.from(b64, "base64"));
  largeXlsx.pageMs = pageMs;
  return file;
}

/// Start recording the longest gap between animation frames (and the longest
/// long task, for the report).
async function startFrames(page) {
  await page.evaluate(() => {
    const g = { max: 0, last: performance.now(), on: true, longest: 0, frames: 0 };
    window.__frames = g;
    const tick = (t) => {
      if (!g.on) return;
      g.max = Math.max(g.max, t - g.last);
      g.last = t;
      g.frames += 1;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    try {
      new PerformanceObserver((list) => {
        for (const e of list.getEntries()) g.longest = Math.max(g.longest, e.duration);
      }).observe({ type: "longtask" });
    } catch {}
  });
}

async function stopFrames(page) {
  return await page.evaluate(() => {
    const g = window.__frames;
    g.on = false;
    return { maxGap: Math.round(g.max), longestTask: Math.round(g.longest), frames: g.frames };
  });
}

test("a large open keeps the editor drawing frames", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const xlsx = await largeXlsx(page);

  await startFrames(page);
  await page.locator("#tb-open").setInputFiles(xlsx);
  await expect(page.locator("#tb-status")).toHaveText(/opened big\.xlsx/, { timeout: 60_000 });
  const m = await stopFrames(page);
  const limit = gapLimit("open", largeXlsx.pageMs);
  console.log(`open of ${ROWS * 5} cells: ${JSON.stringify(m)}; page-thread open ${largeXlsx.pageMs} ms; limit ${limit} ms`);

  // It is the file, and all of it.
  expect(await page.evaluate(async (rows) => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    return [w.session_cell_input(0, 0, 0), w.session_cell_input(0, rows - 1, 3), w.session_format()];
  }, ROWS)).toEqual(["0", `=A${ROWS}*2`, "xlsx"]);
  expect(m.maxGap, `the editor drew no frame for ${m.maxGap} ms while the file opened`).toBeLessThan(limit);
});

test("a large save keeps the editor drawing frames", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  await fillLarge(page);
  const pageMs = await pageThreadMs(page, "save");
  const limit = gapLimit("save", pageMs);

  await startFrames(page);
  const downloaded = page.waitForEvent("download", { timeout: 60_000 });
  await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    // Not awaited: the page is measured while it runs.
    window.__saving = ed.saveAs("xlsx", "download");
  });
  const download = await downloaded;
  await page.evaluate(() => window.__saving);
  const m = await stopFrames(page);
  console.log(`save of ${ROWS * 5} cells: ${JSON.stringify(m)}; page-thread save ${pageMs} ms; limit ${limit} ms`);

  expect(fs.statSync(await download.path()).size).toBeGreaterThan(100_000);
  expect(m.maxGap, `the editor drew no frame for ${m.maxGap} ms while the file saved`).toBeLessThan(limit);
});

test("Excel's progress: the status bar says what is opening, with a progress bar", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const xlsx = await largeXlsx(page);
  await page.locator("#tb-open").setInputFiles(xlsx);
  await expect(page.locator("#tb-status")).toHaveText("Opening 'big.xlsx'");
  await expect(page.locator("#file-progress[role=progressbar]")).toBeVisible();
  await expect(page.locator("#tb-status")).toHaveText(/opened big\.xlsx/, { timeout: 60_000 });
  // Gone when the job is.
  await expect(page.locator("#file-progress")).toHaveCount(0);
});

test("Esc during a large open leaves the workbook exactly as it was", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const xlsx = await largeXlsx(page);
  await page.locator("#tb-open").setInputFiles(xlsx);
  await expect(page.locator("#tb-status")).toHaveText("Opening 'big.xlsx'");
  await page.keyboard.press("Escape");

  await expect(page.locator("#file-progress")).toHaveCount(0);
  const after = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    return { a1: w.session_cell_input(0, 0, 0), a2: w.session_cell_input(0, 1, 0), active: ed.fileJobActive() };
  });
  expect(after, "a cancelled open replaced the workbook that was on screen").toEqual({ a1: "untouched", a2: "", active: false });
  await expect(page.locator("#tb-status")).not.toHaveText(/opened big\.xlsx/);
});

test("Esc during a large save writes nothing and leaves the document unsaved", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  await fillLarge(page);
  await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    ed.wasmApi().session_set_cell(0, 0, 6, "edited");
  });
  let downloads = 0;
  page.on("download", () => { downloads += 1; });
  await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    window.__saving = ed.saveAs("xlsx", "download");
  });
  await expect(page.locator("#tb-status")).toHaveText("Saving 'opencalc.xlsx'");
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.__saving);
  await page.waitForTimeout(500);
  expect(downloads, "a cancelled save still wrote a file").toBe(0);
  expect(await page.evaluate(async () => (await import(window.__editorModule)).isDirty()), "a cancelled save marked the work saved").toBe(true);
});

/// The page does not recalculate a worker-opened workbook to set its clock:
/// the worker did, under the page's clock. So `NOW()` reads today, not
/// 1899-12-30, the moment the file is on screen.
test("a worker open shows the volatile functions under the page's clock", async ({ page }) => {
  await boot(page);
  const r = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    w.session_new();
    w.session_set_cell(0, 0, 0, "=NOW()");
    const bytes = w.session_save_as("xlsx");
    w.session_new();
    const ok = await ed.openBytesAsync(bytes, "clock.xlsx");
    // A delimited save writes the cached values, so this is NOW() as the
    // engine holds it, read without recalculating anything.
    return { ok, now: Number(w.session_save_delimited(0, 44).trim()), today: Date.now() / 86400000 + 25569 };
  });
  expect(r.ok).toBe(true);
  expect(r.now, "NOW() on a worker-opened workbook is not today").toBeGreaterThan(Math.floor(r.today) - 2);
});

/// The bytes must not depend on which thread wrote them.
for (const ext of ["xlsx", "ods", "csv"]) {
  test(`a .${ext} save is byte-identical from the worker and from the page`, async ({ page }) => {
    test.setTimeout(120_000);
    await boot(page);
    // A modest workbook with what a writer has to carry: text, numbers,
    // formulas, a style, a second sheet, a merge.
    await page.evaluate(async () => {
      const ed = await import(window.__editorModule);
      const w = ed.wasmApi();
      w.session_new();
      const lines = [];
      for (let i = 0; i < 3000; i++) lines.push(`${i}\tnamé-${i}\t${(i * 1.1).toFixed(3)}\t=A${i + 1}*2+SUM($A$1:A${i + 1})\t"q,${i}"`);
      w.session_paste_tsv(0, 0, 0, lines.join("\n"));
      try { w.session_add_sheet("Second"); w.session_set_cell(1, 0, 0, "=Sheet1!A10"); } catch {}
      try { w.session_merge(0, 3005, 0, 3006, 2); } catch {}
    });
    const save = async (onWorker) => {
      await page.evaluate(async (on) => (await import(window.__editorModule)).setFileWorkersForTest(on), onWorker);
      const downloaded = page.waitForEvent("download", { timeout: 60_000 });
      await page.evaluate(async (e) => { window.__saving = (await import(window.__editorModule)).saveAs(e, "download"); }, ext);
      // A delimited save of a multi-sheet book asks first.
      if (ext === "csv") await page.getByRole("button", { name: /^Export / }).click();
      await page.evaluate(() => window.__saving);
      return fs.readFileSync(await (await downloaded).path());
    };
    const fromWorker = await save(true);
    const fromPage = await save(false);
    const again = await save(true);
    expect(fromWorker.length).toBeGreaterThan(1000);
    expect(Buffer.compare(fromWorker, fromPage), "the worker wrote different bytes from the page").toBe(0);
    expect(Buffer.compare(again, fromPage), "a second worker save differs").toBe(0);
  });
}

/// No worker — an old browser, a host that blocks them, or a worker script
/// that will not load — and both still work, as they did before.
test("without workers, open and save fall back to the page's thread", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const xlsx = await largeXlsx(page);
  // The worker script cannot be fetched.
  await page.route(/file\.worker\.js/, (route) => route.fulfill({ status: 404, body: "" }));
  await page.locator("#tb-open").setInputFiles(xlsx);
  await expect(page.locator("#tb-status")).toHaveText(/opened big\.xlsx/, { timeout: 60_000 });
  expect(await page.evaluate(async () => (await import(window.__editorModule)).wasmApi().session_cell_input(0, 4, 1))).toBe("name-4");

  // No `Worker` at all.
  await page.evaluate(() => { window.__RealWorker = window.Worker; delete window.Worker; window.Worker = undefined; });
  await page.evaluate(async () => (await import(window.__editorModule)).wasmApi().session_set_cell(0, 0, 9, "edited"));
  const downloaded = page.waitForEvent("download", { timeout: 60_000 });
  await page.evaluate(async () => (await import(window.__editorModule)).saveAs("xlsx", "download"));
  const bytes = fs.readFileSync(await (await downloaded).path());
  expect(bytes.subarray(0, 2).toString(), "the fallback save is not a package").toBe("PK");
  expect(await page.evaluate(async () => (await import(window.__editorModule)).isDirty())).toBe(false);
});

/// The open path's existing guarantees hold on the worker path: the host's
/// read-only setting (`SEC-025`), the import report, and the time budget's
/// "keep waiting" offer (`SEC-017`).
test("a worker open keeps read-only, the import report and the time budget", async ({ page }) => {
  test.setTimeout(120_000);
  await boot(page);
  const xlsx = await largeXlsx(page);
  await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    ed.setTimeBudgetsForTest(0, -1);
  });
  await page.locator("#tb-open").setInputFiles(xlsx);
  await expect(page.locator("#tb-status")).toHaveText(/taking too long/i, { timeout: 60_000 });
  expect(await page.evaluate(async () => (await import(window.__editorModule)).wasmApi().session_cell_input(0, 0, 0))).toBe("untouched");
  await page.locator("#keep-waiting").click();
  await expect(page.locator("#tb-status")).toHaveText(/opened big\.xlsx/, { timeout: 60_000 });

  // The import report is the worker's, carried across: a file whose formula
  // this engine cannot compute is named on open as it is on the page's path.
  const report = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const w = ed.wasmApi();
    const before = w.session_import_summary();
    w.session_new();
    w.session_set_cell(0, 0, 0, "=1+1");
    const bytes = w.session_save_as("xlsx");
    ed.setFileWorkersForTest(false);
    ed.openBytes(bytes, "page.xlsx");
    const onPage = w.session_import_summary();
    ed.setFileWorkersForTest(true);
    await ed.openBytesAsync(bytes, "worker.xlsx");
    return { before, onPage, onWorker: w.session_import_summary() };
  });
  expect(report.onWorker, "the worker open lost the import report").toBe(report.onPage);
  expect(report.before, "the large open's report is not the one being compared").toBeDefined();

  // Read-only from the host survives an open on the worker path (`SEC-025`).
  const r = await page.evaluate(async () => {
    const ed = await import(window.__editorModule);
    const api = ed.wasmApi();
    const bytes = api.session_save_as("xlsx");
    ed.setReadOnly(true);
    const ok = await ed.openBytesAsync(bytes, "again.xlsx");
    let edit;
    try { api.session_set_cell(0, 0, 0, "edited-after-open"); edit = "accepted"; } catch { edit = "refused"; }
    return { ok, edit, readOnly: api.session_read_only() };
  });
  expect(r).toEqual({ ok: true, edit: "refused", readOnly: true });
});

/// A real file opened on the worker is the same session as the same file
/// opened on the page: the same import report, the same format, and the same
/// bytes when it is saved — the save runs the writer over the whole model, so
/// any difference in what crossed shows up there.
for (const name of ["WithChart.xlsx", "StructuredReferences.xlsx", "libreoffice-formulas.xlsx", "sample.xlsx"]) {
  test(`${name} opens to the same session on the worker as on the page`, async ({ page }) => {
    await boot(page);
    const fixture = fs.readFileSync(new URL(`../../fixtures/corpus/${name}`, import.meta.url));
    const r = await page.evaluate(async ({ b64, name }) => {
      const ed = await import(window.__editorModule);
      const w = ed.wasmApi();
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const look = () => [
        w.session_import_summary(),
        w.session_format(),
        w.session_sheet_names(),
        Array.from(w.session_save_native()).join(),
      ];
      ed.setFileWorkersForTest(false);
      const okPage = ed.openBytes(bytes, name);
      const onPage = look();
      ed.setFileWorkersForTest(true);
      const okWorker = await ed.openBytesAsync(bytes, name);
      return { okPage, okWorker, onPage, onWorker: look() };
    }, { b64: fixture.toString("base64"), name });
    expect(r.okPage).toBe(true);
    expect(r.okWorker).toBe(true);
    expect(r.onWorker[0], "import report").toBe(r.onPage[0]);
    expect(r.onWorker[1], "format").toBe(r.onPage[1]);
    expect(r.onWorker[2], "sheets").toBe(r.onPage[2]);
    expect(r.onWorker[3] === r.onPage[3], "the saved bytes differ between a worker open and a page open").toBe(true);
  });
}
