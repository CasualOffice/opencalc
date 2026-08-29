// Every visual defect the user has had to report by hand, as a check.
//
// `ux-sweep.mjs` asks whether a *behaviour* exists. Every defect the user has
// found since then has been visual instead, and the sweep is structurally
// blind to all of them:
//
//   - a dialog's control column collapsed to `0px`, so the range input was an
//     18px pill against the edge and "Style" was clipped off it
//   - eight table-style swatches that all rendered blank, because a UA
//     stylesheet start-aligned four bands to their own empty content height
//   - a table header label drawn underneath its own filter arrow
//   - a table with no outline, in an engine that resolves a border colour
//
// None of those break a behaviour. Every one of them is what a person sees
// first. So this walks the real chrome and measures geometry rather than
// asking whether a command ran.
//
// Run: `python3 webapp/serve.py 8123` then `node ux-visual-audit.mjs [--write]`.

import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";

const PORT = process.env.PORT || 8123;
const PAGE_URL = `http://127.0.0.1:${PORT}/editor.html`;
const MAP = new URL("../../docs/82-UX-VISUAL-AUDIT.md", import.meta.url).pathname;

const findings = [];
const note = (area, where, what, detail) => findings.push({ area, where, what, detail });

// --- the surfaces to walk ---------------------------------------------------
//
// Named rather than discovered, so a surface that stops opening is a visible
// gap in the report instead of silently dropping out of the count.
const SURFACES = [
  ["Create table", (p) => p.evaluate(() => window.opencalcEditor.tableDialog())],
  ["Format cells", (p) => p.evaluate(() => window.opencalcEditor.formatCellsDialog())],
  ["Sort", (p) => p.evaluate(() => window.opencalcEditor.sortDialog())],
  ["Document properties", (p) => p.evaluate(() => window.opencalcEditor.documentPropertiesDialog())],
  ["Custom format", (p) => p.evaluate(() => window.opencalcEditor.customFormatDialog())],
  ["Hyperlink", (p) => p.evaluate(() => window.opencalcEditor.hyperlinkDialog())],
  ["Text to columns", (p) => p.evaluate(() => window.opencalcEditor.textToColumnsDialog())],
  // `pasteSpecialDialog` needs something on the clipboard to be about.
  ["Paste special", async (p) => {
    await p.evaluate(() => { const e = window.opencalcEditor; e.selectForTest(0, 0); e.copySelection?.(); });
    await p.evaluate(() => window.opencalcEditor.pasteSpecialDialog());
  }],
  // `sizeDialog(axis, index)` — bare it opens nothing, which read as a
  // missing surface rather than a wrong call.
  ["Row/column size", (p) => p.evaluate(() => window.opencalcEditor.sizeDialog("col", 0))],
  ["Condition", (p) => p.evaluate(() => window.opencalcEditor.conditionDialog())],
];

// --- the checks, each run over one open surface ------------------------------

/// A grid track that has collapsed to nothing.
///
/// This is what put the create-table range input in the wrong column: a
/// `display: none` child occupies no grid cell, so auto-placement shifts every
/// later child and an `auto` track swells to swallow the row.
const collapsedTracks = () => {
  const out = [];
  const surface = document.querySelector("#oc-modal:not([hidden]) .oc-modal-box, .side-panel:not([hidden])");
  if (!surface) return out;
  for (const el of surface.querySelectorAll("*")) {
    const cs = getComputedStyle(el);
    if (cs.display !== "grid") continue;
    const tracks = cs.gridTemplateColumns.split(/\s+/).map(parseFloat).filter((n) => !Number.isNaN(n));
    if (tracks.length > 1 && tracks.some((t) => t < 1)) {
      out.push({ el: el.className || el.tagName, tracks: cs.gridTemplateColumns });
    }
    const dead = [...el.children].filter((c) => getComputedStyle(c).display === "none").length;
    if (dead) out.push({ el: el.className || el.tagName, hiddenChildren: dead });
  }
  return out;
};

/// Text that does not fit the box drawn for it, in a box that cannot scroll.
///
/// A clipped label is the commonest way a dialog reads as broken, and it is
/// invisible to any test that asks only whether the control works.
const clippedText = () => {
  const out = [];
  const surface = document.querySelector("#oc-modal:not([hidden]) .oc-modal-box, .side-panel:not([hidden])");
  if (!surface) return out;
  for (const el of surface.querySelectorAll("*")) {
    if (el.children.length) continue;
    if (el.closest("[hidden]")) continue;              // leaves only: a container's overflow is its own business
    const cs = getComputedStyle(el);
    if (/auto|scroll/.test(cs.overflowX + cs.overflowY)) continue;
    if (cs.textOverflow === "ellipsis") continue;  // deliberately shortened
    const text = (el.textContent || "").trim();
    if (!text) continue;
    if (el.scrollWidth > el.clientWidth + 1) {
      out.push({ el: el.className || el.tagName, text: text.slice(0, 30), by: el.scrollWidth - el.clientWidth });
    }
  }
  return out;
};

/// An element that carries content and was given no area to draw it in.
///
/// The eight blank table-style swatches were exactly this, one level down:
/// four bands, each 0px tall, inside a button that reserved 6px rows for them.
const zeroSized = () => {
  const out = [];
  // `option` and `optgroup` have no box of their own — the select paints them —
  // and a surface that is closed measures zero throughout. Neither is a defect,
  // and reporting them buries the real ones: the first run of this audit
  // returned 153 findings of which about 140 were these two, which is the same
  // as returning nothing.
  const boxless = new Set(["OPTION", "OPTGROUP", "BR", "SCRIPT", "STYLE", "TEMPLATE"]);
  const surface = document.querySelector("#oc-modal:not([hidden]) .oc-modal-box, .side-panel:not([hidden])");
  if (!surface || surface.getBoundingClientRect().width < 1) return out;
  for (const el of surface.querySelectorAll("*")) {
    if (boxless.has(el.tagName)) continue;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden" || el.hidden) continue;
    if (el.closest("[hidden]")) continue;
    const r = el.getBoundingClientRect();
    const wantsArea = (el.textContent || "").trim()
      || cs.backgroundColor !== "rgba(0, 0, 0, 0)" || cs.borderBottomWidth !== "0px";
    if (wantsArea && (r.width < 1 || r.height < 1)) {
      out.push({ el: el.className || el.tagName, w: Math.round(r.width), h: Math.round(r.height) });
    }
  }
  return out;
};

/// A control drawn outside the surface that owns it.
const escapesContainer = () => {
  const out = [];
  const box = document.querySelector("#oc-modal .oc-modal-box") || document.querySelector(".side-panel");
  if (!box) return out;
  const b = box.getBoundingClientRect();
  for (const el of box.querySelectorAll("input, select, button, label, textarea")) {
    const r = el.getBoundingClientRect();
    if (r.width === 0) continue;
    if (r.right > b.right + 1 || r.left < b.left - 1) {
      out.push({ el: el.className || el.tagName, over: Math.round(Math.max(r.right - b.right, b.left - r.left)) });
    }
  }
  return out;
};

/// A control the pointer cannot comfortably hit.
///
/// 24px is below every platform's own guidance and well below the 44px touch
/// target the mobile work will need; flagged rather than failed, because a
/// dense toolbar is a deliberate trade and a dialog's buttons are not.
const tinyTargets = () => {
  const out = [];
  for (const el of document.querySelectorAll("#oc-modal button, .side-panel button")) {
    // Native checkboxes and radios are 13px by browser default, which is a
    // platform convention rather than this application's choice, and their
    // label is the real target. Buttons are ours.
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    if (el.closest("[hidden]")) continue;
    if (r.height < 24 || r.width < 16) {
      out.push({ el: el.className || el.tagName, w: Math.round(r.width), h: Math.round(r.height) });
    }
  }
  return out;
};

const CHECKS = [
  ["collapsed or mis-placed grid track", collapsedTracks],
  ["text clipped by its own box", clippedText],
  ["content with no area to draw in", zeroSized],
  ["control drawn outside its surface", escapesContainer],
  ["pointer target under 24px", tinyTargets],
];

// --- the canvas checks, which the DOM cannot see -----------------------------

/// A table header label drawn into the space its filter arrow occupies.
///
/// Measured as ink: scan the rightmost strip of each header cell for pixels
/// that are not the header fill, and separate the arrow's own zone from the
/// strip beside it. Glyph ink inside the arrow's zone means the two are drawn
/// on top of each other, which is what "Revenue" reading as "Revenu" is.
async function tableHeaderInk(page) {
  return page.evaluate(() => {
    const ed = window.opencalcEditor;
    const cv = document.querySelector("#grid canvas") || document.querySelector("canvas");
    if (!cv) return [];
    const g = cv.getContext("2d", { willReadFrequently: true });
    const dpr = window.devicePixelRatio || 1;
    let fill = null;
    try { fill = JSON.parse(ed.wasmApi().session_table_at(0, 0, 0)); } catch { return []; }
    if (!fill) return [];
    const [fr, fg, fb] = [0, 2, 4].map((i) => parseInt(fill.headerFill.slice(i, i + 2), 16));
    const out = [];
    for (let c = fill.c0; c <= fill.c1; c += 1) {
      const x0 = ed.colXAt(c), w = ed.colWAt(c);
      if (x0 === undefined) continue;
      const y = Math.round((ed.rowYAt(fill.r0) + ed.rowHAt(fill.r0) / 2) * dpr);
      let inArrow = 0;
      for (let px = Math.round((x0 + w - 9) * dpr); px < Math.round((x0 + w - 2) * dpr); px += 1) {
        const d = g.getImageData(px, y, 1, 1).data;
        if (Math.abs(d[0] - fr) + Math.abs(d[1] - fg) + Math.abs(d[2] - fb) > 90) inArrow += 1;
      }
      if (inArrow > 2) out.push({ col: c, label: fill.cols[c - fill.c0], inkInArrowZone: inArrow });
    }
    return out;
  });
}

/// A table drawn with no outline, in an engine that resolves a border colour.
async function tableOutline(page) {
  return page.evaluate(() => {
    const ed = window.opencalcEditor;
    const cv = document.querySelector("#grid canvas") || document.querySelector("canvas");
    if (!cv) return null;
    const g = cv.getContext("2d", { willReadFrequently: true });
    const dpr = window.devicePixelRatio || 1;
    let t = null;
    try { t = JSON.parse(ed.wasmApi().session_table_at(0, 0, 0)); } catch { return null; }
    if (!t) return null;
    const [br, bg, bb] = [0, 2, 4].map((i) => parseInt(t.border.slice(i, i + 2), 16));
    // Walk the left edge of the table looking for the border colour.
    const x = Math.round(ed.colXAt(t.c0) * dpr);
    let hits = 0;
    for (let r = t.r0; r <= t.r1; r += 1) {
      const y = Math.round((ed.rowYAt(r) + ed.rowHAt(r) / 2) * dpr);
      const d = g.getImageData(x, y, 1, 1).data;
      if (Math.abs(d[0] - br) + Math.abs(d[1] - bg) + Math.abs(d[2] - bb) < 60) hits += 1;
    }
    return { border: t.border, rowsWithBorderInk: hits, rows: t.r1 - t.r0 + 1 };
  });
}

// --- run --------------------------------------------------------------------

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on("pageerror", (e) => note("Console", "page", "an uncaught error", e.message.slice(0, 120)));
// **Native browser dialogs, caught at the call rather than by their absence.**
//
// `sizeDialog` uses `window.prompt` for row height and column width, and this
// audit found it only because a native prompt is not in the DOM — the surface
// read as "did not open". That is luck, not a check. Row height is an everyday
// operation, and a raw prompt cannot be styled, does not look like the
// application, blocks the whole page, and in the desktop shell renders as a
// system dialog carrying the app's URL. Stubbed rather than allowed to open,
// because a real `prompt()` blocks the automation too.
await page.addInitScript(() => {
  window.__nativeDialogs = [];
  for (const name of ["prompt", "confirm", "alert"]) {
    const original = window[name];
    window[name] = (...args) => {
      window.__nativeDialogs.push({ name, message: String(args[0] ?? "").slice(0, 60) });
      return name === "confirm" ? true : name === "prompt" ? "" : undefined;
    };
    window[name].__original = original;
  }
});
await page.goto(PAGE_URL);
await page.waitForFunction(
  () => /^engine v/.test(document.querySelector("#tb-status")?.textContent || ""),
  null, { timeout: 30_000 },
);

// A fresh session, not the sample: `seed()` applies its own header styles and a
// probe that writes over them measures the sample rather than the feature.
await page.evaluate(() => {
  const a = window.opencalcEditor.wasmApi();
  a.session_new();
  const hdr = ["Region", "Rep", "Units", "Revenue"];
  hdr.forEach((h, c) => a.session_set_cell(0, 0, c, h));
  for (let r = 1; r < 9; r += 1) {
    a.session_set_cell(0, r, 0, ["North", "South", "East", "West", "Mid", "Far", "Near", "Top"][r - 1]);
    a.session_set_cell(0, r, 1, `Rep ${r}`);
    a.session_set_cell(0, r, 2, String(r * 7));
    a.session_set_cell(0, r, 3, String(r * 133));
  }
  window.opencalcEditor.selectForTest(0, 0);
});
await page.waitForTimeout(400);

let surfacesOpened = 0;
for (const [name, open] of SURFACES) {
  try {
    await open(page);
    await page.waitForTimeout(300);
    const visible = await page.evaluate(() =>
      !!document.querySelector("#oc-modal:not([hidden]), .side-panel:not([hidden])"));
    if (!visible) { note("Surface", name, "did not open", "no modal or panel became visible"); continue; }
    surfacesOpened += 1;
    for (const [what, fn] of CHECKS) {
      for (const hit of await page.evaluate(fn)) note("Layout", name, what, JSON.stringify(hit));
    }
  } catch (e) {
    note("Surface", name, "threw while opening", e.message.slice(0, 100));
  }
  await page.keyboard.press("Escape").catch(() => {});
  await page.waitForTimeout(150);
}

// Canvas: make a table, then look at it.
await page.evaluate(() => window.opencalcEditor.tableDialog());
await page.waitForTimeout(300);
await page.click("#oc-modal .oc-btn.primary").catch(() => {});
await page.waitForTimeout(600);
await page.evaluate(() => window.opencalcEditor.selectForTest(20, 8));
await page.waitForTimeout(400);

for (const hit of await tableHeaderInk(page)) {
  note("Table", "header row", "the label is drawn into its filter arrow", JSON.stringify(hit));
}
for (const d of await page.evaluate(() => window.__nativeDialogs || [])) {
  note("Chrome", `window.${d.name}`, "a native browser dialog stands in for the application's own",
       d.message || "(no message)");
}

const outline = await tableOutline(page);
if (outline && outline.rowsWithBorderInk === 0) {
  note("Table", "outline", "no border is drawn", `engine resolves ${outline.border} over ${outline.rows} rows`);
}

await browser.close();

// --- report -----------------------------------------------------------------

const byArea = new Map();
for (const f of findings) byArea.set(f.area, [...(byArea.get(f.area) || []), f]);
for (const f of findings) console.log(`${f.area.padEnd(8)} ${f.where.padEnd(22)} ${f.what}  ${f.detail}`);
console.log(`\n${findings.length} finding(s) across ${surfacesOpened} surface(s)`);

if (process.argv.includes("--write")) {
  let out = `<!-- GENERATED by tests/browser/ux-visual-audit.mjs — do not edit by hand.

Regenerate with a served tree:
  python3 webapp/serve.py 8123
  cd tests/browser && node ux-visual-audit.mjs --write
-->

# Visual audit

${findings.length} finding(s) across ${surfacesOpened} surface(s).

\`ux-sweep.mjs\` asks whether a behaviour exists. Every defect a user has
reported by hand here has been visual instead — a collapsed grid track, a blank
swatch, a label under its own control — and a behaviour sweep is blind to all
of them. This walks the real chrome and measures geometry.
`;
  for (const [area, rows] of byArea) {
    out += `\n## ${area}\n\n| where | what | detail |\n|---|---|---|\n`;
    for (const r of rows) out += `| ${r.where} | ${r.what} | \`${r.detail.replace(/\|/g, "\\|")}\` |\n`;
  }
  writeFileSync(MAP, out);
  console.log(`\nwrote ${MAP}`);
}
process.exit(0);
