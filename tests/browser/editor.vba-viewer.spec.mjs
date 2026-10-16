// The Visual Basic window, read-only (`MACRO-05`, docs/99 §4).
//
// Excel's Developer ▸ Code ▸ Visual Basic and Alt+F11 open the Visual Basic
// Editor: a Project Explorer — "VBAProject (Book.xlsm)", then the folders
// "Microsoft Excel Objects", "Forms", "Modules", "Class Modules" — beside the
// selected item's Code window. OpenCalc shows that layout read-only, only when
// the workbook carries a VBA project, with a note that VBA does not run here.
// Viewing changes nothing the save writes: the project goes back out byte for
// byte, untouched and after an edit.
//
// The fixture, `tests/fixtures/vba-hello.xlsm`, carries a real `vbaProject.bin`
// (Compound File + compressed MS-OVBA project) made by the import crate's
// fixture builder — `ThisWorkbook`, `Module1` with `Sub Hello()`, `Class1` —
// and a Rust test holds the committed file to that builder.


import fs from "node:fs";
import zlib from "node:zlib";
import { expect, test } from "@playwright/test";

const FIXTURE = fs.readFileSync(new URL("../fixtures/vba-hello.xlsm", import.meta.url));

/// One part out of a ZIP package: the central directory names it, the local
/// header says where its data starts. Stored or deflated, as an OPC package is.
function zipPart(buf, name) {
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  if (eocd < 0) throw new Error("not a zip");
  const count = buf.readUInt16LE(eocd + 10);
  let at = buf.readUInt32LE(eocd + 16);
  for (let i = 0; i < count; i++) {
    const method = buf.readUInt16LE(at + 10);
    const size = buf.readUInt32LE(at + 20);
    const nameLen = buf.readUInt16LE(at + 28);
    const extra = buf.readUInt16LE(at + 30);
    const comment = buf.readUInt16LE(at + 32);
    const local = buf.readUInt32LE(at + 42);
    const entry = buf.toString("utf8", at + 46, at + 46 + nameLen);
    if (entry === name) {
      const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
      const data = buf.subarray(start, start + size);
      return method === 8 ? zlib.inflateRawSync(data) : Buffer.from(data);
    }
    at += 46 + nameLen + extra + comment;
  }
  return null;
}

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
}

async function openFixture(page) {
  const ok = await page.evaluate((bytes) => window.opencalcEditor.openBytes(new Uint8Array(bytes), "vba-hello.xlsm"), [...FIXTURE]);
  expect(ok, "the fixture is a package this build reads").toBe(true);
  await expect.poll(() => page.evaluate(() => window.opencalcEditor.wasmApi().session_format())).toBe("xlsm");
}


const visualBasic = (page) => page.locator('#rb-panel-developer [data-cmd="developer.visual-basic"]');
const tree = (page) => page.locator("#oc-vba-tree");
const items = (page) => page.locator("#oc-vba-tree .oc-vba-item");
const code = (page) => page.locator("#oc-vba-code");

async function openViewer(page) {
  await page.locator("#rb-tab-developer").click();
  await expect(visualBasic(page)).toBeVisible();
  await visualBasic(page).click();
  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#oc-modal-title")).toHaveText(/^Microsoft Visual Basic for Applications - .+ \[read-only\]$/);
}

/// Save, capturing the bytes rather than downloading them (as
/// `editor.save-formats.spec.mjs` does).
async function saveBytes(page, fmt) {
  const bytes = await page.evaluate(async (fmt) => {
    const realCreate = URL.createObjectURL;
    const realRevoke = URL.revokeObjectURL;
    const realClick = HTMLAnchorElement.prototype.click;
    const blobs = [];
    URL.createObjectURL = (blob) => { blobs.push(blob); return "blob:captured"; };
    URL.revokeObjectURL = () => {};
    HTMLAnchorElement.prototype.click = function () {};
    try {
      await window.opencalcEditor.saveAs(fmt, "download");
      return blobs.length ? [...new Uint8Array(await blobs[0].arrayBuffer())] : null;
    } finally {
      URL.createObjectURL = realCreate;
      URL.revokeObjectURL = realRevoke;
      HTMLAnchorElement.prototype.click = realClick;
    }
  }, fmt);
  expect(bytes, "the save produced a file").not.toBeNull();
  return Buffer.from(bytes);
}


test("Developer ▸ Visual Basic shows the Project Explorer and Sub Hello()", async ({ page }) => {
  await boot(page);
  await openFixture(page);

  // Excel's Code group order: Visual Basic, then Macros.
  await page.locator("#rb-tab-developer").click();
  await expect(page.locator('#rb-panel-developer [data-cmd^="developer."]')).toHaveCount(2);
  await expect(page.locator('#rb-panel-developer [data-cmd^="developer."]').first()).toHaveAttribute("data-cmd", "developer.visual-basic");
  await openViewer(page);

  // The Project Explorer, as the VBA editor draws it.
  await expect(page.locator("#oc-vba-explorer-title")).toHaveText("Project - VBAProject");
  await expect(tree(page).locator('[role="treeitem"] .oc-vba-name')).toHaveText([
    "VBAProject (vba-hello.xlsm)",
    "Microsoft Excel Objects", "ThisWorkbook",
    "Modules", "Module1",
    "Class Modules", "Class1",
  ]);
  expect(await tree(page).locator('[role="treeitem"]').evaluateAll((r) => r.map((n) => n.getAttribute("aria-level")))).toEqual(["1", "2", "3", "2", "3", "2", "3"]);
  await expect(items(page)).toHaveCount(3);
  // An icon per kind (Fluent sprite), as each folder's items carry one.
  expect(await items(page).locator("use").evaluateAll((u) => u.map((n) => n.getAttribute("href"))))
    .toEqual(["#fi-table", "#fi-code", "#fi-shapes"]);

  await items(page).nth(1).click();
  await expect(items(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#oc-vba-code-title")).toHaveText("vba-hello.xlsm - Module1 (Code)");
  // The VBA editor's view: the `Attribute VB_…` header is not shown.
  await expect(code(page)).toHaveValue('Sub Hello()\n    MsgBox "Hello, world"\nEnd Sub\n');
  await expect(code(page)).toHaveAttribute("readonly", "");
  await expect(page.locator(".oc-vba-gutter > div")).toHaveText(["1", "2", "3", "4"]);

  // Arrow keys move through the modules.
  await items(page).nth(1).focus();
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#oc-vba-code-title")).toHaveText("vba-hello.xlsm - Class1 (Code)");
  // Windows-1252, decoded as such: 0x80 is the euro sign.
  await expect(code(page)).toHaveValue(/' café € 5/);

  // Read-only: typing changes nothing.
  await code(page).click();
  await page.keyboard.type("X");
  await expect(code(page)).toHaveValue(/^' café € 5/);

  // The note says VBA does not run and offers the Macros window.
  await expect(page.locator("#oc-vba-note")).toContainText("VBA does not run in OpenCalc");
  await expect(page.locator("#oc-modal .oc-confirm-actions .oc-btn")).toHaveText(["Close", "Copy"]);
  await page.locator("#oc-vba-to-macros").click();
  await expect(page.locator("#oc-modal-title")).toHaveText("Macros");
});

test("Alt+F11 opens it, as in Excel", async ({ page }) => {
  await boot(page);
  await openFixture(page);
  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F11");
  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#oc-modal-title")).toHaveText("Microsoft Visual Basic for Applications - vba-hello.xlsm [read-only]");
});

test("Copy puts the module's code on the clipboard", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "clipboard permissions are a Chromium grant");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await boot(page);
  await openFixture(page);
  await openViewer(page);
  await items(page).nth(1).click();
  await page.locator("#oc-vba-copy").click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("Sub Hello()");
});

test("a workbook without VBA has no Visual Basic, and Alt+F11 opens nothing", async ({ page }) => {
  await boot(page);
  await page.locator("#rb-tab-developer").click();
  await expect(page.locator('#rb-panel-developer [data-cmd="developer.macros"]')).toBeVisible();
  await expect(visualBasic(page)).toBeHidden();
  const refused = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("developer.visual-basic"); return null; } catch (e) { return String(e.message); }
  });
  expect(refused).toMatch(/not available/);
  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F11");
  await expect(page.locator("#oc-modal")).toBeHidden();
});

test("saving keeps vbaProject.bin byte for byte, viewed and edited", async ({ page }) => {
  await boot(page);
  await openFixture(page);
  await openViewer(page);
  await page.locator("#oc-vba-close").click();
  await expect(page.locator("#oc-modal")).toBeHidden();

  const original = zipPart(FIXTURE, "xl/vbaProject.bin");
  expect(original?.length).toBeGreaterThan(512);

  const untouched = await saveBytes(page, "xlsm");
  const keptUntouched = zipPart(untouched, "xl/vbaProject.bin");
  expect(keptUntouched && Buffer.compare(keptUntouched, original), "vbaProject.bin changed on save").toBe(0);

  // Edited: the semantic writer runs, and the project still goes out unchanged.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "42"));
  const edited = await saveBytes(page, "xlsm");
  const kept = zipPart(edited, "xl/vbaProject.bin");
  expect(kept && Buffer.compare(kept, original), "vbaProject.bin changed on an edited save").toBe(0);
});

test("a project that will not decode says why, and is still kept", async ({ page }) => {
  await boot(page);
  // `vba-broken.xlsm`: a macro-enabled package whose `vbaProject.bin` is not a
  // compound file. Visual Basic is still offered — the file does carry a
  // project — and says what is wrong instead of showing nothing.
  const broken = fs.readFileSync(new URL("../fixtures/vba-broken.xlsm", import.meta.url));
  const ok = await page.evaluate((bytes) => window.opencalcEditor.openBytes(new Uint8Array(bytes), "vba-broken.xlsm"), [...broken]);
  expect(ok).toBe(true);
  await openViewer(page);
  await expect(items(page)).toHaveCount(0);
  await expect(page.locator("#oc-vba-problem")).toContainText("could not be read");
  await expect(page.locator("#oc-vba-problem")).toContainText("not a VBA project");
  await page.locator("#oc-vba-close").click();
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 0, 0, "42"));
  const saved = await saveBytes(page, "xlsm");
  expect(Buffer.compare(zipPart(saved, "xl/vbaProject.bin"), zipPart(broken, "xl/vbaProject.bin"))).toBe(0);
});
