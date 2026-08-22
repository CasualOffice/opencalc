// Find, replace, and resolving a typed reference to a place to go.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  A1,
  canvas,
  draw,
  effectiveRange,
  ensureVisible,
  errText,
  findAllSheets,
  findBar,
  findCase,
  findCount,
  findInput,
  findState,
  findValues,
  findWhole,
  findWildcards,
  navBlock,
  on,
  replaceInput,
  select,
  state,
  status,
  statusError,
  switchSheet,
  t,
  updateNameBox,
  usedBounds,
  wasm,
} from "./editor.core.js";

export function colName(n) {
  let s = "";
  n += 1;
  while (n > 0) {
    n -= 1;
    s = String.fromCharCode(65 + (n % 26)) + s;
    n = Math.floor(n / 26);
  }
  return s;
}

export function navTarget() {
  if (navBlock) return navBlock;
  const r = effectiveRange();
  const multi = r.r1 > r.r0 || r.c1 > r.c0;
  return multi ? r : null;
}

export function openFind() { findBar.hidden = false; findInput.focus(); findInput.select(); runFind(); }

export function closeFind() { findBar.hidden = true; canvas.focus(); }

export function runFind() {
  const q = findInput.value;
  findState.matches = q
    ? JSON.parse(wasm.session_find_opts(
        state.sheet, q, findCase.checked,
        findWhole.checked, findValues.checked, findAllSheets.checked,
        findWildcards.checked))
    : [];
  findState.idx = findState.matches.length ? 0 : -1;
  if (findState.idx >= 0) gotoMatch();
  else { findCount.textContent = q ? "0" : ""; draw(); }
}

export function gotoMatch() {
  const m = findState.matches[findState.idx];
  if (!m) return;
  // A whole-workbook search can land on another sheet; follow it there rather
  // than reporting a hit the user cannot see.
  if (m.s !== undefined && m.s !== state.sheet) switchSheet(m.s);
  select(m.r, m.c);
  findCount.textContent = `${findState.idx + 1}/${findState.matches.length}`;
}

export function findStep(dir) {
  if (!findState.matches.length) return;
  findState.idx = (findState.idx + dir + findState.matches.length) % findState.matches.length;
  gotoMatch();
}

export function replaceAll() {
  try {
    const n = wasm.session_replace_all(state.sheet, findInput.value, replaceInput.value, findCase.checked);
    status.textContent = `replaced ${n}`;
  } catch (e) { statusError(errText(e)); }
  runFind();
}

export function replaceOne() {
  const m = findState.matches[findState.idx];
  if (!m || !findInput.value) return;
  try {
    const did = wasm.session_replace_at(state.sheet, m.r, m.c, findInput.value, replaceInput.value, findCase.checked);
    status.textContent = did ? "replaced 1" : "no match here";
  } catch (e) { statusError(errText(e)); }
  runFind();
}

export function colFromLetters(s) {
  let n = 0;
  for (const ch of s.toUpperCase()) {
    if (ch < "A" || ch > "Z") return null;
    n = n * 26 + (ch.charCodeAt(0) - 64);
  }
  return n > 0 ? n - 1 : null;
}

export function parseA1Cell(s) {
  const m = /^([A-Za-z]+)([0-9]+)$/.exec(s.trim());
  if (!m) return null;
  const col = colFromLetters(m[1]);
  const row = parseInt(m[2], 10) - 1;
  return col === null || row < 0 || !Number.isFinite(row) ? null : { row, col };
}

export function parseNameRange(text) {
  const t = (text || "").trim();
  if (!t) return null;
  const b = usedBounds();
  const wholeCols = /^\$?([A-Za-z]{1,3})\s*:\s*\$?([A-Za-z]{1,3})$/.exec(t);
  if (wholeCols) {
    const a = colFromName(wholeCols[1]), z = colFromName(wholeCols[2]);
    if (a === null || z === null) return null;
    return { r0: 0, c0: Math.min(a, z), r1: Math.max(0, b.rows - 1), c1: Math.max(a, z), kind: "cols" };
  }
  const wholeRows = /^\$?(\d+)\s*:\s*\$?(\d+)$/.exec(t);
  if (wholeRows) {
    const a = parseInt(wholeRows[1], 10) - 1, z = parseInt(wholeRows[2], 10) - 1;
    if (a < 0 || z < 0) return null;
    return { r0: Math.min(a, z), c0: 0, r1: Math.max(a, z), c1: Math.max(0, b.cols - 1), kind: "rows" };
  }
  const parts = t.split(":");
  if (parts.length === 2) {
    const p = parseA1Cell(parts[0]), q = parseA1Cell(parts[1]);
    if (!p || !q) return null;
    return {
      r0: Math.min(p.row, q.row), c0: Math.min(p.col, q.col),
      r1: Math.max(p.row, q.row), c1: Math.max(p.col, q.col),
    };
  }
  const c = parseA1Cell(t);
  return c ? { r0: c.row, c0: c.col, r1: c.row, c1: c.col } : null;
}

export function colFromName(letters) {
  let n = 0;
  for (const ch of letters.toUpperCase()) {
    const v = ch.charCodeAt(0) - 64;
    if (v < 1 || v > 26) return null;
    n = n * 26 + v;
  }
  return n - 1;
}

export function gotoName(v) {
  const s = (v || "").trim();
  if (!s) { updateNameBox(); return; }

  // A comma-separated list builds a multi-range selection, as in Excel.
  if (s.includes(",")) {
    const parts = s.split(",").map((x) => x.trim()).filter(Boolean);
    const boxes = parts.map(parseNameRange).filter(Boolean);
    if (boxes.length === parts.length && boxes.length > 1) {
      state.ranges = boxes.slice(0, -1);
      const last = boxes[boxes.length - 1];
      state.anchor = { row: last.r0, col: last.c0 };
      state.sel = { row: last.r1, col: last.c1 };
      state.selKind = "cells";
      ensureVisible();
      draw();
      return;
    }
  }

  // A sheet qualifier moves there first, so `Sheet2!B7` lands on Sheet2.
  let text = s;
  const bang = text.lastIndexOf("!");
  if (bang > 0) {
    const name = text.slice(0, bang).replace(/^'|'$/g, "");
    try {
      const idx = JSON.parse(wasm.session_sheet_names())
        .findIndex((n) => n.toLowerCase() === name.toLowerCase());
      if (idx >= 0) { switchSheet(idx); text = text.slice(bang + 1); }
    } catch {}
  }

  const box = parseNameRange(text);
  if (box) {
    if (box.r0 === box.r1 && box.c0 === box.c1) { select(box.r0, box.c0); return; }
    state.ranges = [];
    state.anchor = { row: box.r0, col: box.c0 };
    state.sel = { row: box.r1, col: box.c1 };
    state.selKind = box.kind || "cells";
    ensureVisible();
    draw();
    return;
  }
  // An existing defined name → jump to its target range.
  try {
    const t = wasm.session_name_target(s);
    if (t !== "null") {
      const r = JSON.parse(t);
      state.anchor = { row: r.r0, col: r.c0 };
      state.sel = { row: r.r1, col: r.c1 };
      state.selKind = "cells";
      state.ranges = [];
      ensureVisible();
      draw();
      return;
    }
  } catch {}
  // A valid new name → define it for the current selection (Excel's name box).
  if (/^[A-Za-z_][A-Za-z0-9_.]*$/.test(s)) {
    const r = effectiveRange();
    const names = JSON.parse(wasm.session_sheet_names());
    const sn = names[state.sheet] || "Sheet1";
    const q = /[^A-Za-z0-9_]/.test(sn) ? `'${sn.replace(/'/g, "''")}'` : sn;
    const refers = `${q}!${A1(r.r0, r.c0)}:${A1(r.r1, r.c1)}`;
    try { wasm.session_define_name(s, refers); status.textContent = `defined name “${s}”`; }
    catch (e) { statusError(errText(e)); }
    updateNameBox();
    return;
  }
  status.textContent = `Can't go to “${s}” — type a cell (B12), range (A1:C5), or a name`;
  updateNameBox();
}
