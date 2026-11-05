// The ribbon's content-parity commands (XL-UX-037, 040, 041, 042, 043).
//
// Excel's Conditional Formatting menu, Function Library, Page Setup, Scale to
// Fit, Sheet Options, Comments, Show, Zoom and Window groups, as commands the
// ribbon (`ribbon.model.js`) and command search can reach. **Every one runs a
// capability the editor or the engine already has** — a page-setup attribute,
// a conditional-format rule kind, the Function Arguments step, the zoom — so
// nothing here is a second implementation, and nothing is a button for a
// feature that does not exist (docs/95 rule 9, docs/91 §8): Excel's controls
// whose features are not built (Icon Sets, A Date Occurring, Red Border,
// Custom Format…, Background, Show Comments, Spelling) are simply absent.
//
// Wording, order and preset colours are Excel for Microsoft 365's, en-US.
// Where Microsoft's documentation does not state them they are from memory of
// the product and marked so in the tracker rows.

import {
  A1,
  autoSum,
  byId,
  canvas,
  chooseFunction,
  draw,
  effectiveRange,
  el,
  ensureVisible,
  errText,
  functionLibrary,
  HH,
  HW,
  insertFunctionDialog,
  manageCfRules,
  messageBox,
  modalFrame,
  openPanel,
  qs,
  resize,
  rowOffsetPx,
  select,
  setFreeze,
  setZoom,
  state,
  status,
  statusError,
  tryEdit,
  wasm,
  ZOOM_MAX,
  ZOOM_MIN,
} from "./editor.core.js";

/* ── Conditional Formatting (XL-UX-037) ─────────────────────────────────── */

/// Excel's "with" presets, in its order: a fill and a text colour each. Red
/// Border and Custom Format… are left out — the engine's rule carries a fill
/// and a font colour, not a border or a full format.
export const CF_PRESETS = [
  ["Light Red Fill with Dark Red Text", "FFC7CE", "9C0006"],
  ["Yellow Fill with Dark Yellow Text", "FFEB9C", "9C5700"],
  ["Green Fill with Dark Green Text", "C6EFCE", "006100"],
  ["Light Red Fill", "FFC7CE", ""],
  ["Red Text", "", "9C0006"],
];

/// The quick-rule dialogs: title, the sentence over the value, and the engine
/// rule kind each one writes.
const QUICK = {
  "greater-than": { title: "Greater Than", lead: "Format cells that are GREATER THAN:", kind: "gt", op: ">" },
  "less-than": { title: "Less Than", lead: "Format cells that are LESS THAN:", kind: "lt", op: "<" },
  "between": { title: "Between", lead: "Format cells that are BETWEEN:", kind: "between" },
  "equal-to": { title: "Equal To", lead: "Format cells that are EQUAL TO:", kind: "eq", op: "=" },
  "text-contains": { title: "Text That Contains", lead: "Format cells that contain the text:", kind: "contains" },
  "duplicate-values": { title: "Duplicate Values", lead: "Format cells that contain:", kind: "duplicate" },
  "top-10-items": { title: "Top 10 Items", lead: "Format cells that rank in the TOP:", kind: "top", rank: true },
  "top-10-percent": { title: "Top 10%", lead: "Format cells that rank in the TOP:", kind: "toppct", rank: true, pct: true },
  "bottom-10-items": { title: "Bottom 10 Items", lead: "Format cells that rank in the BOTTOM:", kind: "bottom", rank: true },
  "bottom-10-percent": { title: "Bottom 10%", lead: "Format cells that rank in the BOTTOM:", kind: "bottompct", rank: true, pct: true },
  "above-average": { title: "Above Average", lead: "Format cells that are ABOVE AVERAGE:", kind: "above", average: true },
  "below-average": { title: "Below Average", lead: "Format cells that are BELOW AVERAGE:", kind: "below", average: true },
};

/// A number typed in the value box, or `null`.
const numberIn = (text) => {
  const t = String(text).trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
};

/// Write one rule over the selection. Returns the engine's refusal, if any.
function addRule(kind, a, b, text, fill, font) {
  const s = effectiveRange();
  try {
    wasm.session_add_cf_styled(state.sheet, s.r0, s.c0, s.r1, s.c1, kind, a, b, text, fill, font);
  } catch (e) { return errText(e); }
  draw();
  return null;
}

/// Excel's small dialog behind each Highlight Cells and Top/Bottom rule: the
/// operand, and the format it paints "with". OK writes one rule over the
/// selection — one undo step, as in Excel.
export function cfQuickRuleDialog(which) {
  const q = QUICK[which];
  if (!q) return;
  const frame = modalFrame(q.title, { onEnter: () => apply() });
  const form = el("div", "oc-cfq");
  form.dataset.rule = which;
  form.appendChild(el("div", "oc-form-label oc-cfq-lead", q.lead));
  const row = el("div", "oc-cfq-row");
  const field = (id, value = "") => {
    const i = el("input", "oc-form-field oc-cfq-value");
    i.type = "text";
    i.id = id;
    i.spellcheck = false;
    i.autocomplete = "off";
    i.value = value;
    i.setAttribute("aria-label", q.lead);
    return i;
  };
  let a = null, b = null, which2 = null;
  if (q.kind === "duplicate") {
    which2 = el("select", "oc-form-field oc-cfq-dup");
    which2.id = "oc-cfq-dup";
    which2.setAttribute("aria-label", q.lead);
    for (const [v, t] of [["duplicate", "Duplicate"], ["unique", "Unique"]]) {
      const o = el("option", null, t); o.value = v; which2.appendChild(o);
    }
    row.append(which2, el("span", "oc-cfq-with", "values with"));
  } else if (q.average) {
    row.append(el("span", "oc-cfq-with", "for the selected range with"));
  } else {
    a = field("oc-cfq-a", q.rank ? "10" : "");
    row.appendChild(a);
    if (q.kind === "between") {
      b = field("oc-cfq-b");
      row.append(el("span", "oc-cfq-with", "and"), b);
    }
    if (q.pct) row.appendChild(el("span", "oc-cfq-with", "%"));
    row.appendChild(el("span", "oc-cfq-with", "with"));
  }
  const preset = el("select", "oc-form-field oc-cfq-preset");
  preset.id = "oc-cfq-preset";
  preset.setAttribute("aria-label", "with");
  CF_PRESETS.forEach(([label], i) => { const o = el("option", null, label); o.value = String(i); preset.appendChild(o); });
  row.appendChild(preset);
  form.appendChild(row);
  const actions = el("div", "oc-confirm-actions");
  const ok = el("button", "oc-btn primary", "OK");
  const cancel = el("button", "oc-btn", "Cancel");
  ok.addEventListener("click", () => apply());
  cancel.addEventListener("click", () => { frame.close(); canvas.focus(); });
  actions.append(ok, cancel);
  frame.body.append(form, actions);
  (a || which2 || preset).focus();
  a?.select();

  function apply() {
    const [, fill, font] = CF_PRESETS[Number(preset.value)] || CF_PRESETS[0];
    let kind = q.kind, av = 0, bv = 0, text = "";
    const s = effectiveRange();
    const anchor = A1(s.r0, s.c0);
    if (q.kind === "duplicate") kind = which2.value;
    else if (q.rank) {
      const n = numberIn(a.value);
      const max = q.pct ? 100 : 1000;
      if (n === null || !Number.isInteger(n) || n < 1 || n > max) {
        messageBox(`Enter an integer between 1 and ${max}.`, { field: a });
        return;
      }
      av = n;
    } else if (q.kind === "contains") {
      text = a.value;
      if (!text) { messageBox("Enter the text to look for.", { field: a }); return; }
    } else if (!q.average) {
      // A number is Excel's cell-value rule. A reference or formula (`=$C$1`)
      // and, for Equal To, a piece of text are the same test written as a
      // formula over the range's top-left cell, which the engine shifts for
      // every other cell — the rule Excel would store, in the one form the
      // engine can hold it in.
      const vals = b ? [a, b] : [a];
      const parsed = vals.map((f) => numberIn(f.value));
      const operand = (f, n) => (n !== null ? String(n) : f.value.trim().startsWith("=") ? `(${f.value.trim().slice(1)})`
        : q.kind === "eq" ? `"${f.value.replace(/"/g, '""')}"` : null);
      const ops = vals.map((f, i) => operand(f, parsed[i]));
      const bad = vals.findIndex((f, i) => !f.value.trim() || ops[i] === null || ops[i] === "()");
      if (bad >= 0) {
        messageBox("Enter a valid value or reference.", { field: vals[bad] });
        return;
      }
      if (parsed.every((n) => n !== null)) {
        av = parsed[0];
        bv = parsed[1] ?? 0;
      } else {
        kind = "formula";
        text = q.kind === "between"
          ? `=AND(${anchor}>=MIN(${ops[0]},${ops[1]}),${anchor}<=MAX(${ops[0]},${ops[1]}))`
          : `=${anchor}${q.op}${ops[0]}`;
      }
    }
    const problem = addRule(kind, av, bv, text, fill, font);
    if (problem) { messageBox(problem, { field: a || preset }); return; }
    frame.close();
    status.textContent = `${CF_PRESETS[Number(preset.value)][0]} — rule applied to ${anchor}${s.r1 > s.r0 || s.c1 > s.c0 ? `:${A1(s.r1, s.c1)}` : ""}`;
    canvas.focus();
  }
}

/// Data Bars › Solid Fill, in Excel's gallery order. The engine draws a solid
/// bar; Gradient Fill is left out until it draws one.
const DATA_BARS = [
  ["blue", "Blue Data Bar", "638EC6"],
  ["green", "Green Data Bar", "63C384"],
  ["red", "Red Data Bar", "FF555A"],
  ["orange", "Orange Data Bar", "FFB628"],
  ["light-blue", "Light Blue Data Bar", "008AEF"],
  ["purple", "Purple Data Bar", "D6007B"],
];

/// Color Scales, in Excel's gallery order. Named high value first, as Excel
/// names them ("Green - Yellow - Red": the highest value green); the engine
/// takes its stops low → high, so the colours are listed in that order.
const GREEN = "63BE7B", YELLOW = "FFEB84", RED = "F8696B", WHITE = "FCFCFF", BLUE = "5A8AC6";
const COLOR_SCALES = [
  ["green-yellow-red", "Green - Yellow - Red Color Scale", [RED, YELLOW, GREEN]],
  ["red-yellow-green", "Red - Yellow - Green Color Scale", [GREEN, YELLOW, RED]],
  ["green-white-red", "Green - White - Red Color Scale", [RED, WHITE, GREEN]],
  ["red-white-green", "Red - White - Green Color Scale", [GREEN, WHITE, RED]],
  ["blue-white-red", "Blue - White - Red Color Scale", [RED, WHITE, BLUE]],
  ["red-white-blue", "Red - White - Blue Color Scale", [BLUE, WHITE, RED]],
  ["white-red", "White - Red Color Scale", [RED, WHITE]],
  ["red-white", "Red - White Color Scale", [WHITE, RED]],
  ["green-white", "Green - White Color Scale", [WHITE, GREEN]],
  ["white-green", "White - Green Color Scale", [GREEN, WHITE]],
  ["green-yellow", "Green - Yellow Color Scale", [YELLOW, GREEN]],
  ["yellow-green", "Yellow - Green Color Scale", [GREEN, YELLOW]],
];
export const CF_DATA_BAR_IDS = DATA_BARS.map(([k]) => `format.cf.data-bar.${k}`);
export const CF_COLOR_SCALE_IDS = COLOR_SCALES.map(([k]) => `format.cf.color-scale.${k}`);

/* ── Page Layout (XL-UX-040) ────────────────────────────────────────────── */

const pageSetup = () => { try { return JSON.parse(wasm.session_page_setup(state.sheet)); } catch { return {}; } };
const setPage = (pairs) => tryEdit(() => wasm.session_set_page_setup(
  state.sheet, Object.keys(pairs), Object.values(pairs).map((v) => String(v))));

/// Excel's three margin presets, in inches: top, bottom, left, right, header,
/// footer.
const MARGINS = [
  ["normal", "Normal", [0.75, 0.75, 0.7, 0.7, 0.3, 0.3]],
  ["wide", "Wide", [1, 1, 1, 1, 0.5, 0.5]],
  ["narrow", "Narrow", [0.75, 0.75, 0.25, 0.25, 0.3, 0.3]],
];
const MARGIN_KEYS = ["top", "bottom", "left", "right", "header", "footer"];
/// The paper sizes the Page Setup panel offers, by OOXML `paperSize`.
const PAPERS = [["1", "Letter"], ["5", "Legal"], ["8", "A3"], ["9", "A4"], ["11", "A5"]];

/// Whether a manual break sits before the active cell's row / column.
function breaksHere() {
  let b = { rows: [], cols: [] };
  try { b = JSON.parse(wasm.session_page_breaks(state.sheet)) || b; } catch { /* none */ }
  const r = effectiveRange();
  return { b, row: r.r0, col: r.c0, hasRow: b.rows.includes(r.r0), hasCol: b.cols.includes(r.c0) };
}
const NONE = 0xffffffff;   // "this axis, not at all" to `session_toggle_page_break`

/// Fit-to width or height: 0 is Automatic, as in OOXML.
function fitValue(axis) {
  const p = pageSetup();
  if (p["setupPr.fitToPage"] !== "1") return 0;
  const v = p[`page.fitTo${axis}`];
  return v === undefined || v === "" ? 1 : Number(v) || 0;
}
function setFit(axis, n) {
  const other = axis === "Width" ? "Height" : "Width";
  const w = axis === "Width" ? n : fitValue("Width");
  const h = axis === "Height" ? n : fitValue("Height");
  if (!w && !h) {
    // Both Automatic: back to scaling, which is what Excel's Scale box then does.
    setPage({ "setupPr.fitToPage": "", "page.fitToWidth": "", "page.fitToHeight": "" });
  } else {
    setPage({ "setupPr.fitToPage": "1", [`page.fitTo${axis}`]: String(n), [`page.fitTo${other}`]: String(axis === "Width" ? h : w), "page.scale": "" });
  }
}
const FIT_VALUES = [{ label: "Automatic", v: "0" }, ...Array.from({ length: 9 }, (_, i) => ({ label: `${i + 1} page${i ? "s" : ""}`, v: String(i + 1) }))];
const fitLabel = (n) => (n ? `${n} page${n > 1 ? "s" : ""}` : "Automatic");

/* ── Review ▸ Comments (XL-UX-042) ──────────────────────────────────────── */

/// Every commented cell on the sheet, in reading order (row, then column).
function commentedCells() {
  let cells = [];
  try { cells = JSON.parse(wasm.session_comments(state.sheet, 0, 0, 1048575, 16383)); } catch { cells = []; }
  return cells.sort((x, y) => x.r - y.r || x.c - y.c);
}
/// Previous / Next Comment: the commented cell before or after the active one,
/// wrapping, with its thread opened beside the grid as Excel opens its card.
function stepComment(dir) {
  const cells = commentedCells();
  if (!cells.length) { status.textContent = "no comments on this sheet"; return; }
  const { row, col } = state.sel;
  const after = (c) => c.r > row || (c.r === row && c.c > col);
  const before = (c) => c.r < row || (c.r === row && c.c < col);
  const next = dir > 0 ? (cells.find(after) || cells[0]) : ([...cells].reverse().find(before) || cells[cells.length - 1]);
  select(next.r, next.c);
  ensureVisible(next.r, next.c);
  openPanel("note");
}
function commentsInSelection() {
  const s = effectiveRange();
  try { return JSON.parse(wasm.session_comments(state.sheet, s.r0, s.c0, s.r1, s.c1)); } catch { return []; }
}

/* ── View (XL-UX-043) ───────────────────────────────────────────────────── */

/// Zoom to Selection: the largest zoom at which the selection fits the grid,
/// then scrolled to it — Excel's "Fit selection".
function zoomToSelection() {
  const s = effectiveRange();
  const box = byId("grid")?.getBoundingClientRect?.() || canvas.getBoundingClientRect();
  const w = wasm.session_col_offset_px(state.sheet, s.c1 + 1) - wasm.session_col_offset_px(state.sheet, s.c0);
  const h = rowOffsetPx(s.r1 + 1) - rowOffsetPx(s.r0);
  if (!(w > 0 && h > 0) || !(box.width > 0 && box.height > 0)) return;
  // The headers scale with the grid, so they are part of what has to fit.
  const z = Math.min(box.width / (HW + w), box.height / (HH + h));
  setZoom(Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, Math.floor(z * 100) / 100)));
  state.scrollX = wasm.session_col_offset_px(state.sheet, s.c0) - wasm.session_col_offset_px(state.sheet, state.freeze?.fc || 0);
  state.scrollY = rowOffsetPx(s.r0) - rowOffsetPx(state.freeze?.fr || 0);
  if (state.scrollX < 0) state.scrollX = 0;
  if (state.scrollY < 0) state.scrollY = 0;
  draw();
}

/// Excel's Zoom dialog: Magnification, as presets, Fit selection or Custom.
export function zoomDialog() {
  const frame = modalFrame("Zoom", { onEnter: () => apply() });
  const form = el("div", "oc-zoom");
  form.appendChild(el("div", "oc-form-label", "Magnification"));
  const current = Math.round(state.zoom * 100);
  const choices = [["200", "200%"], ["100", "100%"], ["75", "75%"], ["50", "50%"], ["25", "25%"], ["fit", "Fit selection"], ["custom", "Custom:"]];
  const radios = [];
  let custom = null;
  for (const [v, label] of choices) {
    const l = el("label", "oc-check oc-zoom-choice");
    const r = document.createElement("input");
    r.type = "radio";
    r.name = "oc-zoom";
    r.value = v;
    r.id = `oc-zoom-${v}`;
    radios.push(r);
    l.append(r, document.createTextNode(` ${label}`));
    if (v === "custom") {
      custom = el("input", "oc-form-field oc-zoom-custom");
      custom.type = "text";
      custom.id = "oc-zoom-custom-value";
      custom.setAttribute("aria-label", "Custom magnification");
      custom.value = String(current);
      custom.addEventListener("focus", () => { r.checked = true; });
      l.append(custom, document.createTextNode(" %"));
    }
    form.appendChild(l);
  }
  (radios.find((r) => r.value === String(current)) || radios[radios.length - 1]).checked = true;
  const actions = el("div", "oc-confirm-actions");
  const ok = el("button", "oc-btn primary", "OK");
  const cancel = el("button", "oc-btn", "Cancel");
  ok.addEventListener("click", () => apply());
  cancel.addEventListener("click", () => { frame.close(); canvas.focus(); });
  actions.append(ok, cancel);
  frame.body.append(form, actions);
  (radios.find((r) => r.checked) || ok).focus();

  function apply() {
    const pick = radios.find((r) => r.checked)?.value;
    if (pick === "fit") { frame.close(); zoomToSelection(); canvas.focus(); return; }
    let pct = Number(pick);
    if (pick === "custom") {
      pct = Number(custom.value.trim());
      const lo = Math.round(ZOOM_MIN * 100), hi = Math.round(ZOOM_MAX * 100);
      if (!Number.isInteger(pct) || pct < lo || pct > hi) {
        messageBox(`Enter a whole number from ${lo} to ${hi}.`, { field: custom });
        return;
      }
    }
    frame.close();
    setZoom(pct / 100);
    canvas.focus();
  }
}

const formulaBar = () => qs(".formula-bar");
const frozen = () => !!(state.freeze && (state.freeze.fr || state.freeze.fc));

/// Register every command above. `R` is the registry's `register`.
export function registerRibbonCommands(R) {
  // Conditional Formatting ▸ Highlight Cells Rules / Top/Bottom Rules.
  for (const [key, label] of [
    ["greater-than", "Greater Than…"], ["less-than", "Less Than…"], ["between", "Between…"],
    ["equal-to", "Equal To…"], ["text-contains", "Text that Contains…"], ["duplicate-values", "Duplicate Values…"],
    ["top-10-items", "Top 10 Items…"], ["top-10-percent", "Top 10%…"], ["bottom-10-items", "Bottom 10 Items…"],
    ["bottom-10-percent", "Bottom 10%…"], ["above-average", "Above Average…"], ["below-average", "Below Average…"],
  ]) {
    R(`format.cf.${key}`, { label, run: () => cfQuickRuleDialog(key) });
  }
  // More Rules… and New Rule… are the rule editor this build has: the
  // conditional-formatting panel, with every rule kind and the formula rule.
  R("format.cf.more-rules", { label: "More Rules…", run: () => openPanel("cf") });
  R("format.cf.new-rule", { label: "New Rule…", run: () => openPanel("cf") });
  R("format.cf.manage-rules", { label: "Manage Rules…", run: () => manageCfRules() });
  for (const [key, label, color] of DATA_BARS) {
    R(`format.cf.data-bar.${key}`, { label, run: () => { const p = addRule("databar", 0, 0, color, "", ""); if (p) statusError(p); } });
  }
  for (const [key, label, stops] of COLOR_SCALES) {
    R(`format.cf.color-scale.${key}`, { label, run: () => { const p = addRule("colorscale", 0, 0, stops.join(","), "", ""); if (p) statusError(p); } });
  }
  // Clear Rules ›. "Selected Cells" removes every rule that touches the
  // selection — the engine's clear, which drops a rule whole rather than
  // trimming its range to the cells outside the selection as Excel does. A
  // rule holds one rectangle, so trimming needs a multi-area range in the
  // model first. Until then the status bar names each rule's whole range, so
  // losing the part outside the selection is said, not silent (Ctrl+Z undoes).
  R("format.cf.clear-selected", { label: "Clear Rules from Selected Cells", run: () => {
    const s = effectiveRange();
    const rules = () => { try { return JSON.parse(wasm.session_cf_rules(state.sheet)); } catch { return []; } };
    const before = rules();
    tryEdit(() => wasm.session_clear_cf(state.sheet, s.r0, s.c0, s.r1, s.c1));
    const left = new Set(rules().map((r) => `${r.range}\u0000${r.desc}`));
    const gone = before.filter((r) => !left.has(`${r.range}\u0000${r.desc}`));
    if (!gone.length) { status.textContent = "no rules on the selected cells"; return; }
    const ranges = [...new Set(gone.map((r) => r.range))].join(", ");
    status.textContent = `removed ${gone.length} ${gone.length === 1 ? "rule" : "rules"}, on ${ranges}`;
  } });
  R("format.cf.clear-sheet", { label: "Clear Rules from Entire Sheet", run: () => {
    tryEdit(() => wasm.session_clear_cf(state.sheet, 0, 0, 1048575, 16383));
  } });

  // Formulas ▸ Function Library (XL-UX-041). Each category drops the
  // implemented functions in it; choosing one opens Function Arguments.
  // Cube is left out of More Functions: the engine implements none of them.
  const library = (category) => async () => {
    const fns = await functionLibrary(category);
    return [
      ...fns.map(({ name, title }) => ({ label: name, title, run: () => chooseFunction(name) })),
      "-",
      { label: "Insert Function…", run: () => insertFunctionDialog() },
    ];
  };
  for (const [key, label, category] of [
    ["recently-used", "Recently Used", "Most Recently Used"],
    ["financial", "Financial", "Financial"], ["logical", "Logical", "Logical"], ["text", "Text", "Text"],
    ["date-time", "Date & Time", "Date & Time"], ["lookup-reference", "Lookup & Reference", "Lookup & Reference"],
    ["math-trig", "Math & Trig", "Math & Trig"],
    ["statistical", "Statistical", "Statistical"], ["engineering", "Engineering", "Engineering"],
    ["information", "Information", "Information"], ["compatibility", "Compatibility", "Compatibility"],
    ["web", "Web", "Web"],
  ]) {
    R(`formulas.library.${key}`, { label, run: () => insertFunctionDialog(), entries: library(category) });
  }
  // AutoSum ›, in Excel's order. More Functions… is Insert Function.
  for (const [key, label, fn] of [["sum", "Sum", "SUM"], ["average", "Average", "AVERAGE"],
    ["count-numbers", "Count Numbers", "COUNT"], ["max", "Max", "MAX"], ["min", "Min", "MIN"]]) {
    R(`formulas.autosum.${key}`, { label, shortcut: key === "sum" ? "Alt+=" : undefined, run: () => autoSum(fn) });
  }
  R("formulas.autosum.more-functions", { label: "More Functions…", run: () => insertFunctionDialog() });

  // Page Layout ▸ Page Setup (XL-UX-040).
  for (const [key, label, values] of MARGINS) {
    R(`page.margins.${key}`, {
      label,
      run: () => setPage(Object.fromEntries(MARGIN_KEYS.map((k, i) => [`margins.${k}`, values[i]]))),
      checked: () => {
        const p = pageSetup();
        return MARGIN_KEYS.every((k, i) => Number(p[`margins.${k}`] ?? MARGINS[0][2][i]) === values[i]);
      },
    });
  }
  R("page.margins.custom", { label: "Custom Margins…", run: () => openPanel("page") });
  for (const [v, label] of [["portrait", "Portrait"], ["landscape", "Landscape"]]) {
    R(`page.orientation.${v}`, {
      label,
      run: () => setPage({ "page.orientation": v }),
      checked: () => (pageSetup()["page.orientation"] || "portrait") === v,
    });
  }
  for (const [v, label] of PAPERS) {
    R(`page.size.${label.toLowerCase()}`, {
      label,
      run: () => setPage({ "page.paperSize": v }),
      checked: () => (pageSetup()["page.paperSize"] || "1") === v,
    });
  }
  R("page.size.more", { label: "More Paper Sizes…", run: () => openPanel("page") });
  R("page.print-area.set", { label: "Set Print Area", run: () => {
    const r = effectiveRange();
    tryEdit(() => wasm.session_set_print_area(state.sheet, r.r0, r.c0, r.r1, r.c1));
  } });
  R("page.print-area.clear", { label: "Clear Print Area", run: () => tryEdit(() => wasm.session_clear_print_area(state.sheet)) });
  // Breaks ›. Insert adds what is not there; Remove takes away what is — the
  // engine's toggle, told which axes to touch.
  R("page.breaks.insert", { label: "Insert Page Break", run: () => {
    const h = breaksHere();
    const row = state.selKind === "cols" || h.hasRow || h.row === 0 ? NONE : h.row;
    const col = state.selKind === "rows" || h.hasCol || h.col === 0 ? NONE : h.col;
    if (row !== NONE || col !== NONE) tryEdit(() => wasm.session_toggle_page_break(state.sheet, row, col));
  } });
  R("page.breaks.remove", {
    label: "Remove Page Break",
    enabled: () => { const h = breaksHere(); return h.hasRow || h.hasCol; },
    run: () => {
      const h = breaksHere();
      if (h.hasRow || h.hasCol) tryEdit(() => wasm.session_toggle_page_break(state.sheet, h.hasRow ? h.row : NONE, h.hasCol ? h.col : NONE));
    },
  });
  R("page.breaks.reset", { label: "Reset All Page Breaks", run: () => {
    const { b } = breaksHere();
    tryEdit(() => {
      for (const r of b.rows) wasm.session_toggle_page_break(state.sheet, r, NONE);
      for (const c of b.cols) wasm.session_toggle_page_break(state.sheet, NONE, c);
    });
  } });
  R("page.print-titles", { label: "Print Titles", run: () => openPanel("page") });
  // Scale to Fit: Width, Height and Scale, as Excel's three boxes.
  for (const axis of ["Width", "Height"]) {
    R(`page.fit.${axis.toLowerCase()}`, {
      label: axis,
      readOnly: true,
      values: () => FIT_VALUES,
      current: () => fitLabel(fitValue(axis)),
      // No value — command search, the SDK's runCommand — is a no-op, as the
      // font, size and number-format combos are; it would otherwise read as 0,
      // Automatic, and rewrite the page setup unasked.
      run: (v) => { if (v === undefined) return; setFit(axis, Math.max(0, parseInt(v, 10) || 0)); },
    });
  }
  R("page.fit.scale", {
    label: "Scale",
    values: () => [10, 25, 50, 75, 100, 125, 150, 200, 300, 400].map((n) => ({ label: `${n}%`, v: String(n) })),
    current: () => `${pageSetup()["page.scale"] || 100}%`,
    // Excel greys Scale out while Width or Height fits the page.
    enabled: () => !fitValue("Width") && !fitValue("Height"),
    run: (v) => {
      if (v === undefined) return;
      const n = parseInt(String(v).replace("%", ""), 10);
      if (!Number.isInteger(n) || n < 10 || n > 400) { statusError("enter a scale from 10% to 400%"); return; }
      setPage({ "page.scale": String(n), "setupPr.fitToPage": "" });
    },
  });
  // Sheet Options: Gridlines and Headings, each to View and to Print.
  R("sheet.gridlines.view", {
    label: "View", title: "View Gridlines",
    checked: () => { try { return !wasm.session_gridlines_hidden(state.sheet); } catch { return true; } },
    run: () => { try { wasm.session_set_gridlines_hidden(state.sheet, !wasm.session_gridlines_hidden(state.sheet)); } catch (e) { statusError(errText(e)); } draw(); },
  });
  R("sheet.headings.view", {
    label: "View", title: "View Headings",
    checked: () => { try { return !wasm.session_headers_hidden(state.sheet); } catch { return true; } },
    run: () => { try { wasm.session_set_headers_hidden(state.sheet, !wasm.session_headers_hidden(state.sheet)); } catch (e) { statusError(errText(e)); } resize(); },
  });
  for (const [key, attr] of [["gridlines", "gridLines"], ["headings", "headings"]]) {
    R(`sheet.${key}.print`, {
      label: "Print", title: `Print ${key === "gridlines" ? "Gridlines" : "Headings"}`,
      checked: () => { const v = pageSetup()[`options.${attr}`]; return v === "1" || v === "true"; },
      run: () => { const p = pageSetup()[`options.${attr}`]; setPage({ [`options.${attr}`]: p === "1" || p === "true" ? "" : "1" }); },
    });
  }

  // Review ▸ Comments (XL-UX-042).
  R("review.delete-comment", {
    label: "Delete Comment",
    enabled: () => commentsInSelection().length > 0,
    run: () => {
      const cells = commentsInSelection();
      if (!cells.length) return;
      tryEdit(() => { for (const c of cells) wasm.session_set_comment(state.sheet, c.r, c.c, "", "", ""); });
      status.textContent = cells.length === 1 ? "comment deleted" : `${cells.length} comments deleted`;
    },
  });
  R("review.previous-comment", { label: "Previous Comment", run: () => stepComment(-1) });
  R("review.next-comment", { label: "Next Comment", run: () => stepComment(1) });

  // View ▸ Show, Zoom and Window (XL-UX-043).
  R("view.show.formula-bar", {
    label: "Formula Bar",
    // A host that hid the formula bar (`?hide=formulabar`) decided it; there
    // is nothing to toggle.
    hidden: () => { const b = formulaBar(); return !b || getComputedStyle(b).display === "none" && b.style.display !== "none"; },
    checked: () => formulaBar()?.style.display !== "none",
    run: () => { const b = formulaBar(); if (!b) return; b.style.display = b.style.display === "none" ? "" : "none"; resize(); },
  });
  R("view.zoom.dialog", { label: "Zoom…", run: () => zoomDialog() });
  R("view.zoom.selection", { label: "Zoom to Selection", run: () => zoomToSelection() });
  R("view.freeze.panes", {
    // Excel's first item becomes Unfreeze Panes while panes are frozen.
    get label() { return frozen() ? "Unfreeze Panes" : "Freeze Panes"; },
    run: () => setFreeze(frozen() ? "none" : "sel"),
  });
  R("view.freeze.top-row-excel", { label: "Freeze Top Row", run: () => setFreeze("row") });
  R("view.freeze.first-column-excel", { label: "Freeze First Column", run: () => setFreeze("col") });
}
