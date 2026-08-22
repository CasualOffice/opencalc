// The pivot panel and the field drag-and-drop that builds a definition.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  PIVOT_AGGREGATES,
  PIVOT_AREAS,
  PIVOT_SORTS,
  activePanel,
  buildPivotPanel,
  byId,
  draw,
  el,
  errText,
  invalidateGrowth,
  off,
  on,
  panelPivot,
  select,
  state,
  status,
  statusError,
  wasm,
} from "./editor.core.js";

export function pivotBlocks(row, col) {
  try { return wasm.session_pivot_blocks(state.sheet, row, col); } catch { return ""; }
}

export function pivotAt(row, col) {
  try {
    return JSON.parse(wasm.session_pivot_at(state.sheet, row, col));
  } catch { return null; }
}

export function currentPivot() {
  if (!panelPivot) return null;
  try {
    const all = JSON.parse(wasm.session_pivots(panelPivot.sheet));
    return all[panelPivot.index] || null;
  } catch { return null; }
}

export function applyPivot(p) {
  try {
    wasm.session_set_pivot(panelPivot.sheet, panelPivot.index, JSON.stringify(p));
    status.textContent = p.values.length ? "pivot refreshed" : "add a field to Values";
  } catch (e) {
    statusError(errText(e));
  }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
}

export function refreshPivotPanel() {
  if (activePanel !== "pivot") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildPivotPanel(body);
}

export function pivotPlacement(p, field) {
  for (const [key] of PIVOT_AREAS) {
    if (p[key].some((f) => f.field === field)) return key;
  }
  return null;
}

export function pivotFieldIsNumeric(p, field) {
  let items = [];
  try {
    items = JSON.parse(wasm.session_pivot_items(panelPivot.sheet, panelPivot.index, field));
  } catch { return false; }
  if (!items.length) return false;
  return items.every((v) => v === "(blank)" || (v.trim() !== "" && !Number.isNaN(Number(v))));
}

export function pivotChip(p, area, f, index) {
  const chip = el("div", "pivot-chip pivot-chip-set");
  chip.draggable = true;
  chip.dataset.index = String(index);
  chip.addEventListener("dragstart", (e) => {
    e.dataTransfer.setData("text/plain", JSON.stringify({ from: area, field: f.field, index }));
    e.dataTransfer.effectAllowed = "move";
  });
  chip.appendChild(el("span", "pivot-chip-name", p.fields[f.field] || `Column${f.field + 1}`));

  if (area === "rows" || area === "cols") {
    const cur = PIVOT_SORTS.findIndex(([v]) => v === (f.sort || "ascending"));
    const [, glyph, hint] = PIVOT_SORTS[cur < 0 ? 0 : cur];
    const sort = el("button", "pivot-mini", glyph);
    sort.title = `Sort: ${hint}`;
    sort.addEventListener("click", () => {
      f.sort = PIVOT_SORTS[((cur < 0 ? 0 : cur) + 1) % PIVOT_SORTS.length][0];
      applyPivot(p);
    });
    chip.appendChild(sort);
    // The innermost field's subtotal would restate the line above it, so it is
    // never emitted — and a switch that does nothing is worse than no switch.
    if (index < p[area].length - 1) {
      const sub = el("button", "pivot-mini" + (f.subtotal ? " on" : ""), "Σ");
      sub.title = f.subtotal ? "Subtotals on" : "Subtotals off";
      sub.addEventListener("click", () => { f.subtotal = !f.subtotal; applyPivot(p); });
      chip.appendChild(sub);
    }
  } else if (area === "values") {
    const agg = el("select", "pivot-agg");
    for (const [value, label] of PIVOT_AGGREGATES) {
      const o = el("option", null, label);
      o.value = value;
      agg.appendChild(o);
    }
    agg.value = f.aggregate || "sum";
    agg.addEventListener("change", () => { f.aggregate = agg.value; applyPivot(p); });
    chip.appendChild(agg);
  } else {
    const shown = !f.selected.length ? "(All)"
      : f.selected.length === 1 ? f.selected[0]
        : `(${f.selected.length} items)`;
    const pick = el("button", "pivot-mini pivot-mini-wide", shown + " ▾");
    pick.title = "Choose which values to include";
    pick.addEventListener("click", () => pivotItemPicker(p, f, chip));
    chip.appendChild(pick);
  }

  const remove = el("button", "pivot-mini pivot-remove", "✕");
  remove.title = "Remove from " + area;
  remove.addEventListener("click", () => { p[area].splice(index, 1); applyPivot(p); });
  chip.appendChild(remove);
  return chip;
}

export function pivotItemPicker(p, f, chip) {
  const open = chip.parentElement.querySelector(".pivot-items");
  if (open) { open.remove(); return; }
  let items = [];
  try {
    items = JSON.parse(wasm.session_pivot_items(panelPivot.sheet, panelPivot.index, f.field));
  } catch { /* an unreadable source lists nothing */ }
  const box = el("div", "pivot-items");
  const chosen = new Set(f.selected);

  const all = el("label", "panel-check");
  const allBox = el("input");
  allBox.type = "checkbox";
  // Empty means every value, which is the `(All)` state — not "none selected".
  allBox.checked = chosen.size === 0;
  allBox.addEventListener("change", () => { f.selected = []; applyPivot(p); });
  all.appendChild(allBox);
  all.appendChild(el("span", null, "(All)"));
  box.appendChild(all);

  for (const item of items) {
    const row = el("label", "panel-check");
    const cb = el("input");
    cb.type = "checkbox";
    cb.checked = chosen.size === 0 || chosen.has(item);
    cb.addEventListener("change", () => {
      const next = chosen.size === 0 ? new Set(items) : new Set(chosen);
      if (cb.checked) next.add(item); else next.delete(item);
      // Everything ticked is the same as nothing chosen, and storing it as
      // `(All)` keeps the pivot following values added to the source later.
      f.selected = next.size === items.length ? [] : [...next];
      applyPivot(p);
    });
    row.appendChild(cb);
    row.appendChild(el("span", null, item));
    box.appendChild(row);
  }
  chip.after(box);
}

export function pivotDropIndex(zone, clientY) {
  const chips = [...zone.querySelectorAll(".pivot-chip-set")];
  for (let i = 0; i < chips.length; i++) {
    const box = chips[i].getBoundingClientRect();
    if (clientY < box.top + box.height / 2) return i;
  }
  return chips.length;
}

export function pivotDrop(p, area, payload, at) {
  if (payload.from === "fields") {
    if (pivotPlacement(p, payload.field)) {
      status.textContent = "that field is already in use — drag it out first";
      return;
    }
    pivotAdd(p, area, payload.field, at);
    return;
  }
  // Moving between (or within) areas: take it out first, then put it back, so
  // a reorder inside one area lands where the pointer says rather than one slot
  // late.
  const [moved] = p[payload.from].splice(payload.index, 1);
  if (!moved) return;
  const index = payload.from === area && payload.index < at ? at - 1 : at;
  pivotInsert(p, area, moved, index);
  applyPivot(p);
}

export function pivotAdd(p, area, field, at) {
  pivotInsert(p, area, { field }, at);
  applyPivot(p);
}

export function pivotInsert(p, area, f, at) {
  const entry = area === "values"
    ? { field: f.field, aggregate: f.aggregate || "sum", name: "", numberFormat: f.numberFormat || null }
    : area === "filters"
      ? { field: f.field, selected: f.selected || [] }
      : { field: f.field, sort: f.sort || "ascending", subtotal: f.subtotal !== false };
  p[area].splice(Math.max(0, Math.min(at, p[area].length)), 0, entry);
}

export function refreshPivotHere() {
  const here = pivotAt(state.sel.row, state.sel.col);
  if (!here) { status.textContent = "no pivot table here"; return; }
  try {
    wasm.session_refresh_pivot(state.sheet, here.index);
    status.textContent = `refreshed ${here.name}`;
  } catch (e) { statusError(errText(e)); }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
}

export function refreshAllPivots() {
  let problems = "";
  try { problems = wasm.session_refresh_all_pivots(); }
  catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  refreshPivotPanel();
  const failed = problems.split("\n").filter(Boolean);
  if (failed.length) statusError(`could not refresh: ${failed.join(", ")}`);
  else status.textContent = "pivots refreshed";
}
