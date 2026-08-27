// Modal dialogs, side panels and context menus.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  A1,
  A1range,
  BD_TITLES,
  COLOR_PALETTE,
  COL_W,
  ROW_H,
  TABLE_STYLES,
  TOTALS_FUNCTIONS,
  activePanel,
  afterFilterChange,
  applyPersonalFilter,
  applySort,
  applyTableStyle,
  autofitColumn,
  autofitRow,
  borderColor,
  borderStyle,
  byId,
  canvas,
  cellRef,
  clearAll,
  clearFormats,
  clearSelection,
  closePanel,
  colName,
  colXAt,
  colors,
  commit,
  ctx,
  currentTable,
  doCopy,
  doCut,
  doPaste,
  doPasteMode,
  draw,
  effectiveRange,
  el,
  errText,
  formatSel,
  gotoName,
  invalidateGrowth,
  locale,
  looksLikeHeader,
  ocOverlayHost,
  off,
  on,
  openPanel,
  panelNote,
  panelRangeEls,
  parseColor,
  printSheet,
  pushRecent,
  readThread,
  recentColors,
  renameSheet,
  renderTabs,
  resetView,
  rowHAt,
  rowYAt,
  selRect,
  select,
  setNumberFormat,
  shiftIsRisky,
  sortRange,
  sortTarget,
  state,
  status,
  statusError,
  switchSheet,
  t,
  tabsEl,
  tintColor,
  toggleFilter,
  tryEdit,
  validationChevron,
  wasm,
  wrap,
} from "./editor.core.js";

export function manageCfRules() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Conditional formatting rules";

  const close = () => { modal.hidden = true; body.textContent = ""; };
  const render = () => {
    body.textContent = "";
    let rules = [];
    try { rules = JSON.parse(wasm.session_cf_rules(state.sheet)); } catch {}
    if (!rules.length) {
      body.append(el("p", "oc-confirm-text", "No conditional formatting on this sheet."));
    } else {
      body.append(el("p", "oc-confirm-text", "Listed in evaluation order — the first rule that matches a cell wins."));
      const list = el("div", "cf-rules");
      for (const r of rules) {
        const row = el("div", "cf-rule-row");
        const sw = el("span", "cf-rule-swatch");
        if (r.fill) sw.style.background = "#" + r.fill;
        const label = el("span", "cf-rule-text", `${r.range} — ${r.desc}`);
        const stop = document.createElement("input");
        stop.type = "checkbox";
        stop.checked = !!r.stop;
        stop.title = "Stop evaluating later rules when this one matches";
        stop.addEventListener("change", () => {
          tryEdit(() => wasm.session_set_cf_stop(state.sheet, r.i, stop.checked));
          render();
        });
        const up = el("button", "oc-btn", "↑");
        up.title = "Evaluate earlier";
        up.addEventListener("click", () => { tryEdit(() => wasm.session_reorder_cf_rule(state.sheet, r.i, true)); render(); });
        const down = el("button", "oc-btn", "↓");
        down.title = "Evaluate later";
        down.addEventListener("click", () => { tryEdit(() => wasm.session_reorder_cf_rule(state.sheet, r.i, false)); render(); });
        const del = el("button", "oc-btn", "Delete");
        del.addEventListener("click", () => { tryEdit(() => wasm.session_delete_cf_rule(state.sheet, r.i)); render(); });
        row.append(sw, label, stop, up, down, del);
        list.appendChild(row);
      }
      body.appendChild(list);
    }
    const actions = el("div", "oc-confirm-actions");
    const done = el("button", "oc-btn primary", "Close");
    done.addEventListener("click", () => { close(); canvas.focus(); });
    actions.appendChild(done);
    body.appendChild(actions);
    done.focus();
  };
  render();
  modal.hidden = false;
}

export function formatCellsDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Format cells";
  body.textContent = "";

  let cur = {};
  try { cur = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col)) || {}; }
  catch {}

  const tabs = el("div", "fc-tabs");
  const pages = el("div", "fc-pages");
  const made = [];
  const addTab = (name, build) => {
    const b = el("button", "fc-tab", name);
    const page = el("div", "fc-page");
    page.hidden = made.length > 0;
    if (!made.length) b.classList.add("on");
    build(page);
    b.addEventListener("click", () => {
      for (const [tb, pg] of made) { tb.classList.remove("on"); pg.hidden = true; }
      b.classList.add("on");
      page.hidden = false;
    });
    tabs.appendChild(b);
    pages.appendChild(page);
    made.push([b, page]);
  };

  // Each page collects its own setter, applied together on OK so the whole
  // dialog is one visible change rather than a dozen.
  const pending = [];

  addTab("Number", (page) => {
    page.append(el("p", "oc-confirm-text", "Format code"));
    const inp = el("input", "cf-code");
    inp.value = cur.nf || "";
    inp.placeholder = "General";
    inp.spellcheck = false;
    const preview = el("div", "cf-preview");
    const render = () => {
      try { preview.textContent = inp.value.trim() ? wasm.format_preview(1234.567, inp.value.trim()) : "1234.567"; }
      catch { preview.textContent = "—"; }
    };
    inp.addEventListener("input", render);
    render();
    const presets = el("div", "cf-presets");
    for (const [label, code] of [
      ["General", ""], ["0.00", "0.00"], ["#,##0", "#,##0"], ["0%", "0%"],
      ["$#,##0.00", "$#,##0.00"], ["yyyy-mm-dd", "yyyy-mm-dd"], ["Text", "@"],
    ]) {
      const b = el("button", "cf-preset", label);
      b.addEventListener("click", () => { inp.value = code; render(); });
      presets.appendChild(b);
    }
    page.append(inp, preview, presets);
    pending.push((s) => wasm.session_set_number_format(state.sheet, s.r0, s.c0, s.r1, s.c1, inp.value.trim()));
  });

  addTab("Font", (page) => {
    const row = el("div", "fc-row");
    const mk = (label, on) => {
      const l = el("label", "fc-check");
      const c = document.createElement("input");
      c.type = "checkbox";
      c.checked = !!on;
      l.append(c, document.createTextNode(" " + label));
      row.appendChild(l);
      return c;
    };
    const b = mk("Bold", cur.b), i = mk("Italic", cur.i);
    const u = mk("Underline", cur.u), st = mk("Strikethrough", cur.st);
    page.append(row);
    page.append(el("p", "oc-confirm-text", "Size (pt)"));
    const size = el("input", "panel-field");
    size.type = "number"; size.min = "1"; size.max = "409";
    size.value = cur.fs || "";
    size.placeholder = "default";
    page.append(size);
    page.append(el("p", "oc-confirm-text", "Text colour"));
    const col = document.createElement("input");
    col.type = "color";
    col.value = cur.fc ? "#" + cur.fc : "#000000";
    page.append(col);
    pending.push((s) => {
      wasm.session_set_font_flags(
        state.sheet, s.r0, s.c0, s.r1, s.c1,
        b.checked, i.checked, u.checked, st.checked);
      if (size.value) wasm.session_set_font_size(state.sheet, s.r0, s.c0, s.r1, s.c1, parseFloat(size.value));
      wasm.session_set_font_color(state.sheet, s.r0, s.c0, s.r1, s.c1, col.value.replace("#", ""), -1, 0);
    });
  });

  addTab("Alignment", (page) => {
    page.append(el("p", "oc-confirm-text", "Horizontal"));
    const h = el("select", "panel-select");
    for (const [v, t] of [["", "General"], ["left", "Left"], ["center", "Center"], ["right", "Right"],
                          ["fill", "Fill"], ["justify", "Justify"],
                          ["centerContinuous", "Center across selection"], ["distributed", "Distributed"]]) {
      const o = el("option", null, t); o.value = v; h.appendChild(o);
    }
    h.value = cur.al || "";
    const v = el("select", "panel-select");
    for (const [val, t] of [["", "Default"], ["top", "Top"], ["middle", "Middle"], ["bottom", "Bottom"],
                            ["justify", "Justify"], ["distributed", "Distributed"]]) {
      const o = el("option", null, t); o.value = val; v.appendChild(o);
    }
    v.value = { t: "top", m: "middle", b: "bottom", vj: "justify", vd: "distributed" }[cur.va] || "";
    const wrapL = el("label", "fc-check");
    const wrapC = document.createElement("input");
    wrapC.type = "checkbox"; wrapC.checked = !!cur.w;
    wrapL.append(wrapC, document.createTextNode(" Wrap text"));
    page.append(el("p", "oc-confirm-text", "Horizontal"), h,
                el("p", "oc-confirm-text", "Vertical"), v, wrapL);
    pending.push((s) => {
      wasm.session_set_align(state.sheet, s.r0, s.c0, s.r1, s.c1, h.value);
      wasm.session_set_valign(state.sheet, s.r0, s.c0, s.r1, s.c1, v.value);
      wasm.session_set_text_overflow(state.sheet, s.r0, s.c0, s.r1, s.c1, wrapC.checked ? "wrap" : "overflow");
    });
  });

  addTab("Fill", (page) => {
    page.append(el("p", "oc-confirm-text", "Background"));
    const col = document.createElement("input");
    col.type = "color";
    col.value = cur.bg ? "#" + cur.bg : "#ffffff";
    const none = el("button", "cf-preset", "No fill");
    let cleared = false;
    none.addEventListener("click", () => { cleared = true; none.classList.add("on"); });
    col.addEventListener("input", () => { cleared = false; none.classList.remove("on"); });
    page.append(col, none);
    pending.push((s) =>
      wasm.session_set_fill(state.sheet, s.r0, s.c0, s.r1, s.c1, cleared ? "" : col.value.replace("#", ""), -1, 0));
  });

  addTab("Border", (page) => {
    page.append(el("p", "oc-confirm-text", "Placement"));
    const grid = el("div", "fc-borders");
    let chosen = null;
    for (const kind of ["all", "outer", "inner", "top", "bottom", "left", "right",
                        "topandbottom", "bottomdouble", "diagdown", "diagup", "none"]) {
      const b = el("button", "cf-preset", BD_TITLES[kind] || kind);
      b.addEventListener("click", () => {
        chosen = kind;
        grid.querySelectorAll("button").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
      });
      grid.appendChild(b);
    }
    page.append(grid);
    page.append(el("div", "panel-hint", "Uses the line style and colour from the toolbar's border palette."));
    pending.push((s) => {
      if (chosen) wasm.session_set_border(state.sheet, s.r0, s.c0, s.r1, s.c1, chosen, borderStyle, borderColor);
    });
  });

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(tabs, pages, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const s = effectiveRange();
    close();
    canvas.focus();
    // One try around the lot: a failure part-way through should report once, not
    // once per tab.
    try { for (const apply of pending) apply(s); }
    catch (e) { statusError(errText(e)); }
    draw();
  });
  ok.focus();
}

export function cellStyleGallery() {
  let styles = [];
  try { styles = JSON.parse(wasm.session_cell_styles()); } catch {}
  if (!styles.length) { status.textContent = "no cell styles available"; return; }

  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Cell styles";
  body.textContent = "";
  body.append(el("p", "oc-confirm-text", "Applies the style's formatting and tags the cells with its name."));

  const grid = el("div", "style-gallery");
  const close = () => { modal.hidden = true; body.textContent = ""; };
  for (const st of styles) {
    const b = el("button", "style-chip", st.n);
    // Preview each entry in its own look, which is the only way to tell
    // "Heading 2" from "Heading 3" at a glance.
    if (st.bg) b.style.background = "#" + st.bg;
    if (st.fg) b.style.color = "#" + st.fg;
    if (st.bold) b.style.fontWeight = "700";
    if (st.sz) b.style.fontSize = Math.min(18, Math.max(11, st.sz)) + "px";
    b.addEventListener("click", () => {
      close();
      canvas.focus();
      formatSel((r) => wasm.session_apply_cell_style(state.sheet, r.r0, r.c0, r.r1, r.c1, st.n));
      status.textContent = `applied cell style "${st.n}"`;
    });
    grid.appendChild(b);
  }
  body.appendChild(grid);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Close");
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  actions.appendChild(cancel);
  body.appendChild(actions);
  modal.hidden = false;
  grid.querySelector("button")?.focus();
}

export function buildColorMenu(menu, onPick, noneLabel) {
  menu.textContent = "";
  // `link` is the theme slot + tint a swatch came from, or null for a colour
  // with no theme behind it. Carrying it through to the model is what lets a
  // themed cell follow the workbook when the palette is changed elsewhere; a
  // theme swatch stored as bare RRGGBB is indistinguishable from a hand-picked
  // colour and stays put forever.
  const pick = (hex, link) => { pushRecent(hex); onPick(hex, link || null); menu.hidden = true; canvas.focus(); };
  const none = el("button", "cm-none");
  // oc-safe-html: a literal SVG icon plus `noneLabel`, which is a UI string
  // from this module or the host's i18n table — never workbook text.
  // oc-safe-html: see the note above.
  none.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="icon-sm"><circle cx="12" cy="12" r="9"/><line x1="5.6" y1="5.6" x2="18.4" y2="18.4"/></svg>' +
    `<span>${noneLabel}</span>`;
  none.addEventListener("click", (e) => { e.stopPropagation(); pick(""); });
  menu.appendChild(none);

  // `links[i]`, when given, is the theme slot + tint that produced `colors[i]`.
  const grid = (colors, links) => {
    const g = el("div", "cm-grid");
    colors.forEach((c, i) => {
      const b = el("button", "cm-sw");
      b.style.background = "#" + c;
      b.title = "#" + c;
      const link = links && links[i];
      b.addEventListener("click", (e) => { e.stopPropagation(); pick(c, link); });
      g.appendChild(b);
    });
    return g;
  };
  if (recentColors.length) {
    menu.appendChild(el("div", "cm-label", "Recent"));
    menu.appendChild(grid(recentColors));
  }
  // The workbook's own theme, not a stock imitation of one: the engine hands
  // back the slots it read from `theme1.xml`. Slot order is OOXML's, and the
  // first four are the light/dark background/text pairs — shown in the order
  // Excel shows them so the swatches sit where people expect.
  let theme = [];
  try { theme = JSON.parse(wasm.theme_colors()); } catch {}
  if (theme.length >= 10) {
    const order = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    const slots = order.filter((i) => theme[i]);
    const base = slots.map((i) => theme[i]);
    menu.appendChild(el("div", "cm-label", "Theme"));
    menu.appendChild(grid(base, slots.map((slot) => ({ slot, tint: 0 }))));
    // Excel's tint ladder under the base row: lighter above, darker below. A
    // tinted swatch stays linked to its slot — the tint is part of the
    // reference, not a way out of it.
    for (const t of [0.6, 0.4, -0.25, -0.5]) {
      menu.appendChild(grid(
        base.map((c) => tintColor(c, t)),
        slots.map((slot) => ({ slot, tint: t })),
      ));
    }
  }
  menu.appendChild(el("div", "cm-label", "Standard"));
  menu.appendChild(grid(COLOR_PALETTE));

  menu.appendChild(el("div", "cm-label", "Custom"));
  const custom = el("div", "cm-custom");
  const hex = el("input", "cm-hex");
  hex.placeholder = "#RRGGBB";
  hex.spellcheck = false;
  hex.addEventListener("click", (e) => e.stopPropagation());
  const apply = el("button", "cm-apply", "Apply");
  const commitHex = () => {
    const parsed = parseColor(hex.value);
    if (parsed) pick(parsed);
    else { hex.style.borderColor = "#e5484d"; }
  };
  hex.addEventListener("keydown", (e) => { if (e.key === "Enter") { e.stopPropagation(); commitHex(); } });
  hex.addEventListener("input", () => { hex.style.borderColor = ""; });
  apply.addEventListener("click", (e) => { e.stopPropagation(); commitHex(); });
  custom.appendChild(hex);
  custom.appendChild(apply);
  menu.appendChild(custom);

  // Native colour dialog — the full HS/V surface, without shipping one.
  const more = el("div", "cm-custom");
  const native = el("input", "cm-native");
  native.type = "color";
  native.title = "More colours";
  native.addEventListener("click", (e) => e.stopPropagation());
  native.addEventListener("change", (e) => { e.stopPropagation(); pick(native.value.replace("#", "").toUpperCase()); });
  more.appendChild(native);
  // Eyedropper is Chromium-only, so it appears only where it works rather than
  // sitting there dead.
  if (window.EyeDropper) {
    const drop = el("button", "cm-apply", "Pick from screen");
    drop.addEventListener("click", async (e) => {
      e.stopPropagation();
      try {
        const { sRGBHex } = await new window.EyeDropper().open();
        const parsed = parseColor(sRGBHex);
        if (parsed) pick(parsed);
      } catch {
        // The user dismissed the picker; nothing to report.
      }
    });
    more.appendChild(drop);
  }
  menu.appendChild(more);
}

export function customFormatDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Custom number format";
  body.textContent = "";

  let current = "";
  let sample = 1234.567;
  let sampleText = "";
  try {
    const fmt = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col));
    current = fmt.nf || "";
    const it = JSON.parse(wasm.session_cells(state.sheet, state.sel.row, state.sel.col, state.sel.row, state.sel.col))[0];
    if (it && it.n) sample = parseFloat(wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col)) || sample;
    else if (it && it.t) sampleText = it.t;
  } catch {}

  const input = document.createElement("input");
  input.type = "text";
  input.className = "cf-code";
  input.spellcheck = false;
  input.value = current;
  input.placeholder = "#,##0.00;[Red]-#,##0.00";
  const preview = el("div", "cf-preview");
  const hint = el("p", "cf-hint",
    "Sections are separated by ; — positive;negative;zero;text. [Red] colours a section, @ stands for the text value.");

  const render = () => {
    const code = input.value.trim();
    try {
      preview.textContent = code
        ? (sampleText
            ? wasm.format_preview_text(sampleText, code)
            : wasm.format_preview(sample, code))
        : String(sampleText || sample);
      preview.classList.remove("bad");
    } catch {
      preview.textContent = "—";
      preview.classList.add("bad");
    }
  };
  input.addEventListener("input", render);
  render();

  const presets = el("div", "cf-presets");
  for (const [label, code] of [
    ["Red negatives", "#,##0.00;[Red]-#,##0.00"],
    ["Thousands", "#,##0"],
    ["Scientific", "0.00E+00"],
    ["Accounting-ish", "$#,##0.00;[Red]($#,##0.00);\"-\""],
    ["Text", "@"],
    ["Suffix", "0\" kg\""],
  ]) {
    const b = el("button", "cf-preset", label);
    b.title = code;
    b.addEventListener("click", () => { input.value = code; render(); input.focus(); });
    presets.appendChild(b);
  }

  // Currency builder. A currency format is `[$SYM-locale]`, and the locale id is
  // a hex LCID nobody remembers — so the code is assembled rather than typed.
  // The symbol goes *inside* the bracket: writing a bare "£" would work until it
  // met a format that treats the character as a literal in the wrong section.
  const CURRENCIES = [
    ["$", "409", "US dollar"],
    ["£", "809", "Pound sterling"],
    ["€", "407", "Euro"],
    ["¥", "411", "Japanese yen"],
    ["₹", "4009", "Indian rupee"],
    ["CHF", "807", "Swiss franc"],
    ["A$", "C09", "Australian dollar"],
    ["C$", "1009", "Canadian dollar"],
  ];
  const curWrap = el("div", "cf-currency");
  const curSel = document.createElement("select");
  curSel.className = "panel-select";
  curSel.setAttribute("aria-label", "Currency");
  for (const [sym, lcid, name] of CURRENCIES) {
    const o = el("option", null, `${sym} — ${name}`);
    o.value = `${sym}|${lcid}`;
    curSel.appendChild(o);
  }
  const decSel = document.createElement("select");
  decSel.className = "panel-select";
  decSel.setAttribute("aria-label", "Decimal places");
  for (const d of [0, 2]) {
    const o = el("option", null, d === 0 ? "no decimals" : "2 decimals");
    o.value = String(d);
    decSel.appendChild(o);
  }
  decSel.value = "2";
  const redNeg = document.createElement("input");
  redNeg.type = "checkbox";
  const redLabel = el("label", "cf-redneg");
  redLabel.append(redNeg, document.createTextNode(" red negatives"));
  const build = el("button", "cf-preset", "Insert currency format");
  build.addEventListener("click", () => {
    const [sym, lcid] = curSel.value.split("|");
    const dp = decSel.value === "0" ? "" : ".00";
    const money = `[$${sym}-${lcid}]#,##0${dp}`;
    input.value = redNeg.checked ? `${money};[Red]-${money}` : money;
    render();
    input.focus();
  });
  curWrap.append(curSel, decSel, redLabel, build);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(el("p", "oc-confirm-text", "Format code"), input, preview,
              el("p", "oc-confirm-text", "Currency"), curWrap, presets, hint, actions);
  modal.hidden = false;
  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", close);
  ok.addEventListener("click", () => { const code = input.value.trim(); close(); canvas.focus(); setNumberFormat(code); });
  input.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter") ok.click();
    else if (e.key === "Escape") { close(); canvas.focus(); }
  });
  input.focus();
  input.select();
}

export function sortDialog() {
  const s = sortTarget();
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Sort range";
  body.textContent = "";

  const where = el("p", "oc-confirm-text",
    `${colName(s.c0)}${s.r0 + 1}:${colName(s.c1)}${s.r1 + 1} — ${s.r1 - s.r0 + 1} rows`);
  const headerRow = el("label", "sort-head");
  const headerBox = document.createElement("input");
  headerBox.type = "checkbox";
  headerBox.checked = looksLikeHeader(s);
  headerRow.append(headerBox, document.createTextNode(" My data has a header row"));

  const keysWrap = el("div", "sort-keys");
  const cols = [];
  for (let c = s.c0; c <= s.c1; c += 1) cols.push(c);
  const headingOf = (c) => {
    if (!headerBox.checked) return colName(c);
    try {
      const it = JSON.parse(wasm.session_cells(state.sheet, s.r0, c, s.r0, c))[0];
      if (it && it.t) return `${colName(c)} — ${it.t}`;
    } catch {}
    return colName(c);
  };
  const rows = [];
  const addKeyRow = (index) => {
    const row = el("div", "sort-key");
    row.append(el("span", "sort-lbl", index === 0 ? "Sort by" : "Then by"));
    const pick = document.createElement("select");
    const none = document.createElement("option");
    none.value = ""; none.textContent = "—";
    if (index > 0) pick.appendChild(none);
    for (const c of cols) {
      const o = document.createElement("option");
      o.value = String(c);
      o.textContent = headingOf(c);
      pick.appendChild(o);
    }
    pick.value = String(index === 0 ? Math.min(Math.max(state.sel.col, s.c0), s.c1) : "");
    const dir = document.createElement("select");
    for (const [v, t] of [["asc", "A → Z"], ["desc", "Z → A"]]) {
      const o = document.createElement("option");
      o.value = v; o.textContent = t;
      dir.appendChild(o);
    }
    row.append(pick, dir);
    keysWrap.appendChild(row);
    rows.push({ pick, dir });
  };
  [0, 1, 2].forEach(addKeyRow);
  // Re-label the pickers when the header checkbox flips, so they name the
  // columns the way the user now thinks of them.
  headerBox.addEventListener("change", () => {
    for (const { pick } of rows) {
      [...pick.options].forEach((o) => { if (o.value !== "") o.textContent = headingOf(+o.value); });
    }
  });

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Sort");
  actions.append(cancel, ok);
  body.append(where, headerRow, keysWrap, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", close);
  ok.addEventListener("click", () => {
    const keys = rows
      .filter(({ pick }) => pick.value !== "")
      .map(({ pick, dir }) => ({ col: +pick.value, asc: dir.value === "asc" }));
    close();
    canvas.focus();
    if (keys.length) applySort(s, keys, headerBox.checked);
  });
  ok.focus();
}

export function openColumnFilter(col, x, y) {
  closeSheetMenu();
  let payload;
  try { payload = JSON.parse(wasm.session_filter_values(state.sheet, col)); }
  catch { status.textContent = "could not read column values"; return; }
  const all = payload.values || [];

  // Working set of checked values, seeded from what the engine reports.
  const checked = new Set(all.filter((v) => v.c).map((v) => v.v));

  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu filter-menu";
  menu.id = "sheet-ctx";
  menu.addEventListener("click", (e) => e.stopPropagation());

  const head = document.createElement("div");
  head.className = "menu-label";
  head.textContent = `Filter ${colName(col)}`;
  menu.appendChild(head);

  // Conditions — the entry point to the two-comparison dialog.
  const cond = document.createElement("button");
  cond.className = "menu-item filter-cond";
  cond.textContent = payload.custom ? "Edit condition…" : "Filter by condition…";
  cond.addEventListener("click", () => { closeSheetMenu(); conditionDialog(col); });
  menu.appendChild(cond);

  if (payload.custom) {
    const note = document.createElement("div");
    note.className = "panel-hint";
    note.textContent = "A condition is active on this column. Ticking values below replaces it.";
    menu.appendChild(note);
  }
  if (payload.truncated) {
    const note = document.createElement("div");
    note.className = "panel-hint";
    note.textContent = `Only the first ${all.length} distinct values are listed — use a condition to match the rest.`;
    menu.appendChild(note);
  }

  const search = document.createElement("input");
  search.type = "search";
  search.className = "filter-search";
  search.placeholder = "Search values";
  search.setAttribute("aria-label", `Search values in ${colName(col)}`);
  menu.appendChild(search);

  const allRow = document.createElement("label");
  allRow.className = "filter-item filter-all";
  const allCb = document.createElement("input");
  allCb.type = "checkbox";
  allRow.appendChild(allCb);
  allRow.appendChild(document.createTextNode("(Select all)"));
  menu.appendChild(allRow);

  const list = document.createElement("div");
  list.className = "filter-list";
  menu.appendChild(list);

  // Rebuild the visible rows for the current search text. (Select all) applies
  // to what is *shown*, which is what makes search-then-tick usable.
  let shown = all;
  function build() {
    const q = search.value.trim().toLowerCase();
    shown = q ? all.filter((v) => v.v.toLowerCase().includes(q)) : all;
    list.textContent = "";
    for (const item of shown) {
      const row = document.createElement("label");
      row.className = "filter-item";
      const cb = document.createElement("input");
      cb.type = "checkbox";
      cb.checked = checked.has(item.v);
      cb.addEventListener("change", () => {
        if (cb.checked) checked.add(item.v); else checked.delete(item.v);
        syncAll();
      });
      row.appendChild(cb);
      row.appendChild(document.createTextNode(item.v === "" ? "(Blanks)" : item.v));
      list.appendChild(row);
    }
    if (!shown.length) {
      const none = document.createElement("div");
      none.className = "panel-hint";
      none.textContent = "No matching values";
      list.appendChild(none);
    }
    syncAll();
  }
  function syncAll() {
    const on = shown.filter((v) => checked.has(v.v)).length;
    allCb.checked = shown.length > 0 && on === shown.length;
    allCb.indeterminate = on > 0 && on < shown.length;
  }
  allCb.addEventListener("change", () => {
    for (const v of shown) { if (allCb.checked) checked.add(v.v); else checked.delete(v.v); }
    build();
  });
  search.addEventListener("input", build);
  build();

  // Whose filter is this? docs/71: the choice is offered, and defaults to
  // shared, because shared is what a spreadsheet has always done and the only
  // one the file format can express. "Just for me" never touches the document —
  // no operation on the wire, nothing in the undo history, nothing saved, and
  // the SUBTOTAL underneath does not move.
  const scope = document.createElement("label");
  scope.className = "filter-scope";
  const mine = document.createElement("input");
  mine.type = "checkbox";
  mine.className = "filter-scope-box";
  scope.appendChild(mine);
  scope.appendChild(document.createTextNode(" Just for me"));
  const scopeHint = document.createElement("div");
  scopeHint.className = "panel-hint";
  scopeHint.textContent = "Others keep seeing every row.";
  scopeHint.hidden = true;
  mine.addEventListener("change", () => { scopeHint.hidden = !mine.checked; });
  menu.appendChild(scope);
  menu.appendChild(scopeHint);

  const foot = document.createElement("div");
  foot.className = "filter-foot";
  const clr = document.createElement("button");
  clr.className = "filter-clear";
  clr.textContent = "Clear";
  clr.addEventListener("click", () => {
    closeSheetMenu();
    // Clearing drops this participant's view *and* the shared rule, because
    // "Clear" on a column means the column is not filtering — and a user who
    // cannot tell which of the two hid a row cannot be asked to clear the right
    // one.
    if (wasm.session_has_personal_view(state.sheet)) {
      try { wasm.session_clear_personal_view(state.sheet); } catch {}
    }
    tryEdit(() => wasm.session_set_filter_values(state.sheet, col, []));
    afterFilterChange();
  });
  const apply = document.createElement("button");
  apply.className = "filter-apply";
  apply.textContent = "Apply";
  apply.addEventListener("click", () => {
    closeSheetMenu();
    // Everything ticked means "no rule" — same as clearing, and it keeps the
    // saved file free of a filter that excludes nothing.
    const values = all.every((v) => checked.has(v.v)) ? [] : all.filter((v) => checked.has(v.v)).map((v) => v.v);
    if (values.length === 0 && !all.every((v) => checked.has(v.v))) {
      status.textContent = "tick at least one value";
      return;
    }
    if (mine.checked) {
      // Personal: ask the engine which rows this value-set hides, then keep
      // them in the session's own view. Deliberately *not* `tryEdit` — this is
      // not an edit, and routing it through one is the mistake docs/71 exists
      // to prevent.
      applyPersonalFilter(col, values);
    } else {
      tryEdit(() => wasm.session_set_filter_values(state.sheet, col, values));
    }
    afterFilterChange();
  });
  foot.appendChild(clr);
  foot.appendChild(apply);
  menu.appendChild(foot);

  positionMenu(menu, x, y);
  search.focus();
}

export function conditionDialog(col) {
  const OPS = [
    ["equal", "equals"],
    ["notEqual", "does not equal"],
    ["greaterThan", "is greater than"],
    ["greaterThanOrEqual", "is greater than or equal to"],
    ["lessThan", "is less than"],
    ["lessThanOrEqual", "is less than or equal to"],
    ["contains", "contains"],
    ["notContains", "does not contain"],
    ["beginsWith", "begins with"],
    ["endsWith", "ends with"],
  ];
  // The last four are not OOXML operators. Excel stores them as equal /
  // notEqual with wildcards, so translate here and keep the written file honest
  // rather than inventing an operator no other reader would understand.
  const encode = (op, val) => {
    switch (op) {
      case "contains": return ["equal", `*${val}*`];
      case "notContains": return ["notEqual", `*${val}*`];
      case "beginsWith": return ["equal", `${val}*`];
      case "endsWith": return ["equal", `*${val}`];
      default: return [op, val];
    }
  };

  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Filter by condition";
  body.textContent = "";

  const where = el("p", "oc-confirm-text", `Show rows where ${colName(col)}:`);
  const mkRow = (n) => {
    const row = el("div", "filter-cond-row");
    const sel = document.createElement("select");
    sel.setAttribute("aria-label", `Condition ${n} operator`);
    if (n === 2) sel.append(new Option("(none)", ""));
    for (const [v, label] of OPS) sel.append(new Option(label, v));
    const inp = document.createElement("input");
    inp.type = "text";
    inp.setAttribute("aria-label", `Condition ${n} value`);
    row.append(sel, inp);
    return { row, sel, inp };
  };
  const one = mkRow(1);
  const two = mkRow(2);

  const join = el("div", "filter-join");
  const radios = [];
  for (const [val, text] of [["and", "And"], ["or", "Or"]]) {
    const l = el("label");
    const r = document.createElement("input");
    r.type = "radio";
    r.name = "oc-filter-join";
    r.value = val;
    if (val === "and") r.checked = true;
    radios.push(r);
    l.append(r, document.createTextNode(" " + text));
    join.append(l);
  }

  const hint = el("div", "panel-hint", "Wildcards: * matches any characters, ? matches one.");
  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Apply");
  actions.append(cancel, ok);
  body.append(where, one.row, join, two.row, hint, actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const [op1, v1] = encode(one.sel.value, one.inp.value);
    if (!one.inp.value) { status.textContent = "enter a value to compare against"; one.inp.focus(); return; }
    let op2 = "", v2 = "";
    if (two.sel.value && two.inp.value) [op2, v2] = encode(two.sel.value, two.inp.value);
    const and = radios.find((r) => r.checked).value === "and";
    close();
    canvas.focus();
    tryEdit(() => wasm.session_set_filter_custom(state.sheet, col, op1, v1, op2, v2, and));
    afterFilterChange();
  });
  one.inp.focus();
}

export function openValidationMenu() {
  if (!validationChevron) return;
  const rect = canvas.getBoundingClientRect();
  const x = rect.left + validationChevron.x;
  const y = rect.top + validationChevron.y + validationChevron.h;
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu dv-menu";
  menu.id = "sheet-ctx";
  validationChevron.values.forEach((val) => {
    const b = document.createElement("button");
    b.textContent = val;
    b.addEventListener("click", () => {
      closeSheetMenu();
      try { wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, val); }
      catch (e) { statusError(errText(e)); }
      draw();
    });
    menu.appendChild(b);
  });
  positionMenu(menu, x, y);
}

export function panelLabel(body, text) { body.appendChild(el("div", "panel-section-label", text)); }

export function panelRangeReadout(body) {
  panelLabel(body, "Apply to range");
  const r = el("div", "panel-range", A1range(effectiveRange()));
  body.appendChild(r);
  panelRangeEls.push(r);
}

export function panelActions(body, primaryText, onPrimary, ghostText, onGhost) {
  const row = el("div", "panel-actions");
  const ghost = el("button", "panel-btn-ghost", ghostText);
  ghost.addEventListener("click", onGhost);
  const primary = el("button", "panel-btn-primary", primaryText);
  primary.addEventListener("click", onPrimary);
  row.appendChild(ghost);
  row.appendChild(primary);
  body.appendChild(row);
  return primary;
}

export function buildTablePanel(body) {
  const t = currentTable();
  if (!t) {
    body.appendChild(el("div", "panel-note", "Select a cell inside a table."));
    return;
  }
  const at = () => ({ r: t.r0, c: t.c0 });

  panelLabel(body, "Name");
  const name = el("input", "panel-field");
  name.type = "text";
  name.value = t.name;
  // On commit rather than per keystroke: a half-typed name is usually invalid,
  // and rejecting it mid-word would fight the person typing.
  const rename = () => {
    const want = name.value.trim();
    if (!want || want === t.name) { name.value = t.name; return; }
    try {
      wasm.session_rename_table(state.sheet, at().r, at().c, want);
      status.textContent = `renamed to ${want}`;
    } catch (e) {
      statusError(errText(e));
      name.value = t.name;
    }
    draw();
    refreshTablePanel();
  };
  name.addEventListener("change", rename);
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.stopPropagation(); name.blur(); }
    else if (e.key === "Escape") { e.stopPropagation(); name.value = t.name; name.blur(); }
  });
  body.appendChild(name);

  panelLabel(body, "Range");
  body.appendChild(el("div", "panel-range", `${A1(t.r0, t.c0)}:${A1(t.r1, t.c1)}`));

  panelLabel(body, "Style");
  const styles = el("div", "oc-table-styles");
  for (const [label, id] of TABLE_STYLES) {
    const b = el("button", "oc-style-swatch" + (id === t.style ? " sel" : ""));
    b.type = "button";
    b.title = label;
    // Swatches are painted from the engine's own resolution, so the preview
    // and the grid cannot disagree about what a style looks like.
    let c = { headerFill: "FFFFFF", bandFill: "F2F2F2", border: "BFBFBF" };
    try { c = JSON.parse(wasm.session_table_style_preview(id)) || c; } catch {}
    const head = el("span");
    head.style.background = "#" + c.headerFill;
    head.style.borderBottom = "2px solid #" + c.border;
    const band = el("span");
    band.style.background = "#" + c.bandFill;
    b.append(head, el("span"), band, el("span"));
    b.addEventListener("click", () => applyTableStyle({ style: id }));
    styles.appendChild(b);
  }
  body.appendChild(styles);

  panelLabel(body, "Show");
  const checks = el("div", "oc-table-checks");
  const check = (label, on, onChange) => {
    const l = el("label", "oc-check");
    const i = document.createElement("input");
    i.type = "checkbox";
    i.checked = on;
    i.addEventListener("change", () => onChange(i.checked));
    l.append(i, document.createTextNode(" " + label));
    checks.appendChild(l);
  };
  check("Header row", t.headers > 0, (on) => {
    tryEdit(() => wasm.session_set_table_headers(state.sheet, at().r, at().c, on));
    refreshTablePanel();
  });
  check("Totals row", t.totals > 0, (on) => {
    tryEdit(() => wasm.session_table_totals(state.sheet, at().r, at().c, on));
    refreshTablePanel();
  });
  check("Banded rows", !!t.stripes, (on) => applyTableStyle({ stripes: on }));
  check("Banded columns", !!t.colStripes, (on) => applyTableStyle({ colStripes: on }));
  check("First column", !!t.firstCol, (on) => applyTableStyle({ firstCol: on }));
  check("Last column", !!t.lastCol, (on) => applyTableStyle({ lastCol: on }));
  body.appendChild(checks);

  // One picker per column, only while there is a totals row to put a result
  // in. Choosing a function writes the SUBTOTAL the choice means — recording
  // the choice alone leaves the row blank here and in Excel.
  if (t.totals > 0) {
    panelLabel(body, "Totals row");
    let funcs = [];
    try {
      funcs = JSON.parse(wasm.session_totals_functions(state.sheet, at().r, at().c));
    } catch {}
    const grid = el("div", "oc-totals-grid");
    for (let c = t.c0; c <= t.c1; c++) {
      const i = c - t.c0;
      const lab = el("span", "oc-totals-col", (t.cols && t.cols[i]) || A1(t.r0, c));
      const sel = el("select", "panel-select");
      for (const [label, id] of TOTALS_FUNCTIONS) {
        const o = document.createElement("option");
        o.value = id;
        o.textContent = label;
        if (id === (funcs[i] || "")) o.selected = true;
        sel.appendChild(o);
      }
      sel.addEventListener("change", () => {
        tryEdit(() => wasm.session_set_totals_function(state.sheet, t.r1, c, sel.value));
        refreshTablePanel();
      });
      grid.append(lab, sel);
    }
    body.appendChild(grid);
  }

  const row = el("div", "panel-actions");
  const rm = el("button", "panel-btn-ghost", "Convert to range");
  rm.addEventListener("click", async () => {
    const cur = currentTable();
    if (!cur) return;
    const ok = await confirmModal(
      `Convert "${cur.name}" to a range`,
      "The values and formatting stay. The table's name goes, so any formula "
        + "written as " + cur.name + "[Column] will stop resolving.",
      "Convert to range",
    );
    if (!ok) return;
    tryEdit(() => wasm.session_remove_table(state.sheet, cur.r0, cur.c0));
    status.textContent = "converted to a range";
    closePanel();
  });
  row.appendChild(rm);
  body.appendChild(row);
}

export function refreshTablePanel() {
  if (activePanel !== "table") return;
  const body = byId("side-panel-body");
  body.textContent = "";
  buildTablePanel(body);
}

export function buildPagePanel(body) {
  const get = () => { try { return JSON.parse(wasm.session_page_setup(state.sheet)); } catch { return {}; } };
  let cur = get();
  const set = (pairs) => {
    tryEdit(() => wasm.session_set_page_setup(
      state.sheet, Object.keys(pairs), Object.values(pairs).map((v) => String(v))));
    cur = get();
  };

  panelLabel(body, "Orientation");
  const orient = el("select", "panel-select");
  for (const [v, t] of [["portrait", "Portrait"], ["landscape", "Landscape"]]) {
    const o = el("option", null, t); o.value = v; orient.appendChild(o);
  }
  orient.value = cur["page.orientation"] || "portrait";
  orient.addEventListener("change", () => set({ "page.orientation": orient.value }));
  body.appendChild(orient);

  panelLabel(body, "Paper");
  const paper = el("select", "panel-select");
  // `paperSize` is a numbered enum; these are the sizes people actually pick.
  for (const [v, t] of [["1", "Letter"], ["5", "Legal"], ["8", "A3"], ["9", "A4"], ["11", "A5"]]) {
    const o = el("option", null, t); o.value = v; paper.appendChild(o);
  }
  paper.value = cur["page.paperSize"] || "1";
  paper.addEventListener("change", () => set({ "page.paperSize": paper.value }));
  body.appendChild(paper);

  panelLabel(body, "Scale");
  const scaleWrap = el("div", "oc-totals-grid");
  const scale = el("input", "panel-field");
  scale.type = "number"; scale.min = "10"; scale.max = "400";
  scale.value = cur["page.scale"] || "100";
  scale.addEventListener("change", () => set({
    "page.scale": scale.value,
    // Scaling and fit-to-page are alternatives in Excel; setting one has to
    // clear the other or the file asks for both and the reader picks.
    "setupPr.fitToPage": "",
  }));
  scaleWrap.append(el("span", "oc-totals-col", "Percent"), scale);
  const fitW = el("input", "panel-field");
  fitW.type = "number"; fitW.min = "0";
  fitW.value = cur["page.fitToWidth"] || "1";
  const fitH = el("input", "panel-field");
  fitH.type = "number"; fitH.min = "0";
  fitH.value = cur["page.fitToHeight"] || "1";
  const fitOn = () => set({
    "setupPr.fitToPage": "1",
    "page.fitToWidth": fitW.value,
    "page.fitToHeight": fitH.value,
    "page.scale": "",
  });
  fitW.addEventListener("change", fitOn);
  fitH.addEventListener("change", fitOn);
  scaleWrap.append(el("span", "oc-totals-col", "Fit to width"), fitW);
  scaleWrap.append(el("span", "oc-totals-col", "Fit to height"), fitH);
  body.appendChild(scaleWrap);

  panelLabel(body, "Margins (inches)");
  const mg = el("div", "oc-totals-grid");
  for (const [key, label, dflt] of [
    ["top", "Top", "0.75"], ["bottom", "Bottom", "0.75"],
    ["left", "Left", "0.7"], ["right", "Right", "0.7"],
  ]) {
    const i = el("input", "panel-field");
    i.type = "number"; i.step = "0.05"; i.min = "0";
    i.value = cur["margins." + key] || dflt;
    i.addEventListener("change", () => set({ ["margins." + key]: i.value }));
    mg.append(el("span", "oc-totals-col", label), i);
  }
  body.appendChild(mg);

  panelLabel(body, "Print");
  const checks = el("div", "oc-table-checks");
  const check = (label, key, group) => {
    const l = el("label", "oc-check");
    const i = document.createElement("input");
    i.type = "checkbox";
    i.checked = cur[group + "." + key] === "1" || cur[group + "." + key] === "true";
    i.addEventListener("change", () => set({ [group + "." + key]: i.checked ? "1" : "" }));
    l.append(i, document.createTextNode(" " + label));
    checks.appendChild(l);
  };
  check("Gridlines", "gridLines", "options");
  check("Row/column headings", "headings", "options");
  check("Centre across", "horizontalCentered", "options");
  check("Centre down", "verticalCentered", "options");
  body.appendChild(checks);

  panelLabel(body, "What prints");
  let scope = {};
  try { scope = JSON.parse(wasm.session_print_scope(state.sheet)); } catch {}
  const scopeRow = (label, current, onSet, onClear) => {
    body.appendChild(el("div", "panel-range", current || "(all of it)"));
    const row = el("div", "panel-actions");
    const clear = el("button", "panel-btn-ghost", "Clear");
    clear.addEventListener("click", () => { onClear(); openPanel("page"); });
    const set = el("button", "panel-btn-ghost", label);
    set.addEventListener("click", () => { onSet(); openPanel("page"); });
    row.append(clear, set);
    body.appendChild(row);
  };
  panelLabel(body, "Print area");
  scopeRow(
    "Set from selection",
    scope.area,
    () => { const r = effectiveRange();
      tryEdit(() => wasm.session_set_print_area(state.sheet, r.r0, r.c0, r.r1, r.c1)); },
    () => tryEdit(() => wasm.session_clear_print_area(state.sheet)),
  );
  panelLabel(body, "Repeat rows at the top");
  scopeRow(
    "Set from selection",
    scope.titles,
    () => { const r = effectiveRange();
      tryEdit(() => wasm.session_set_print_title_rows(state.sheet, r.r0, r.r1)); },
    // r1 < r0 clears it — the engine's own signal for "no titles".
    () => tryEdit(() => wasm.session_set_print_title_rows(state.sheet, 1, 0)),
  );

  panelLabel(body, "Header and footer");
  for (const [key, ph] of [["oddHeader", "Header"], ["oddFooter", "Footer"]]) {
    const i = el("input", "panel-field");
    i.placeholder = ph + " — &L left, &C centre, &R right, &P page";
    i.value = cur["hf." + key] || "";
    i.addEventListener("change", () => set({ ["hf." + key]: i.value }));
    body.appendChild(i);
  }

  panelActions(body, "Print…", () => printSheet(), "Close", () => closePanel());
}

export function togglePanel(tool) {
  if (activePanel === tool) closePanel();
  else openPanel(tool);
}

export function buildDvPanel(body) {
  panelRangeReadout(body);
  panelLabel(body, "Allow");
  // Every OOXML kind, not just the dropdown. The other kinds constrain what may
  // be typed; only `list` shows a picker.
  const kindSel = el("select", "panel-select");
  for (const [v, t] of [
    ["list", "List of values"], ["whole", "Whole number"], ["decimal", "Number"],
    ["date", "Date"], ["time", "Time"], ["textLength", "Text length"],
    ["custom", "Custom formula"], ["none", "Any value"],
  ]) { const o = el("option", null, t); o.value = v; kindSel.appendChild(o); }
  body.appendChild(kindSel);

  // List values.
  const inp = el("input", "panel-field");
  inp.placeholder = "Yes, No, Maybe";
  inp.spellcheck = false;
  const s0 = effectiveRange();
  try {
    const vj = wasm.session_validation_at(state.sheet, s0.r0, s0.c0);
    if (vj !== "null") inp.value = JSON.parse(vj).join(", ");
  } catch {}
  const listHint = el("div", "panel-hint", "Comma-separated. Cells in the range show a dropdown to pick from these values.");

  // Comparison operands, for the kinds that take them.
  const opSel = el("select", "panel-select");
  for (const [v, t] of [
    ["between", "between"], ["notBetween", "not between"], ["equal", "equal to"],
    ["notEqual", "not equal to"], ["greaterThan", "greater than"], ["lessThan", "less than"],
    ["greaterThanOrEqual", "at least"], ["lessThanOrEqual", "at most"],
  ]) { const o = el("option", null, t); o.value = v; opSel.appendChild(o); }
  const f1 = el("input", "panel-field"); f1.placeholder = "value"; f1.spellcheck = false;
  const f2 = el("input", "panel-field"); f2.placeholder = "and"; f2.spellcheck = false;
  const customHint = el("div", "panel-hint", "A formula that must be true, e.g. A1>0. Checked by the calc engine, not here.");
  body.append(inp, listHint, opSel, f1, f2, customHint);

  panelLabel(body, "If the value is rejected");
  // `stop` is the only style that actually refuses the entry; the other two
  // let the value through. Carrying the attribute without offering it turned
  // every advisory rule in an opened file into a hard block.
  const styleSel = el("select", "panel-select");
  for (const [v, t] of [
    ["stop", "Stop — refuse the value"],
    ["warning", "Warning — allow, but say so"],
    ["information", "Information — allow, with a note"],
  ]) { const o = el("option", null, t); o.value = v; styleSel.appendChild(o); }
  const errTitle = el("input", "panel-field");
  errTitle.placeholder = "Title (optional)";
  errTitle.spellcheck = false;
  const msg = el("input", "panel-field");
  msg.placeholder = "Optional message";
  msg.spellcheck = false;
  const blankWrap = el("label", "panel-check");
  const blank = document.createElement("input");
  blank.type = "checkbox";
  blank.checked = true;
  blankWrap.append(blank, document.createTextNode(" allow an empty cell"));
  const hideWrap = el("label", "panel-check");
  const hideDrop = document.createElement("input");
  hideDrop.type = "checkbox";
  hideWrap.append(hideDrop, document.createTextNode(" no in-cell dropdown"));
  body.append(styleSel, errTitle, msg, blankWrap, hideWrap);

  panelLabel(body, "Hint shown when the cell is selected");
  const promptTitle = el("input", "panel-field");
  promptTitle.placeholder = "Title (optional)";
  promptTitle.spellcheck = false;
  const promptText = el("input", "panel-field");
  promptText.placeholder = "e.g. Pick a region from the list";
  promptText.spellcheck = false;
  body.append(promptTitle, promptText);

  // Load whatever the cell's existing rule says, so the panel edits the rule
  // rather than silently replacing its wording with blanks on Apply.
  try {
    const j = wasm.session_validation_messages(state.sheet, s0.r0, s0.c0);
    if (j) {
      const m = JSON.parse(j);
      styleSel.value = m.style || "stop";
      errTitle.value = m.errorTitle || "";
      msg.value = m.errorText || "";
      promptTitle.value = m.promptTitle || "";
      promptText.value = m.promptText || "";
      hideDrop.checked = !!m.hideDropdown;
    }
  } catch {}

  const sync = () => {
    const k = kindSel.value;
    const isList = k === "list";
    const isCustom = k === "custom";
    const isNone = k === "none";
    const cmp = !isList && !isCustom && !isNone;
    inp.style.display = isList ? "" : "none";
    listHint.style.display = isList ? "" : "none";
    opSel.style.display = cmp ? "" : "none";
    f1.style.display = cmp || isCustom ? "" : "none";
    f2.style.display = cmp && opSel.value.toLowerCase().includes("between") ? "" : "none";
    customHint.style.display = isCustom ? "" : "none";
    f1.placeholder = isCustom ? "A1>0" : k === "textLength" ? "length" : "value";
  };
  kindSel.addEventListener("change", sync);
  opSel.addEventListener("change", sync);
  sync();

  const apply = panelActions(
    body,
    "Apply",
    () => {
      const s = effectiveRange();
      try {
        if (kindSel.value === "list") {
          // Excel's own dialog takes either a comma list or a range in this one
          // field, and a range is what most real dropdowns use — kept out of
          // the way on another sheet and maintained on its own. A leading `=`
          // is how Excel spells it; a bare `A1:A9` is accepted too, because
          // that is what people type when they forget.
          const typed = inp.value.trim();
          const looksLikeRange =
            typed.startsWith("=") ||
            (!typed.includes(",") && /^(?:'[^']+'|[A-Za-z0-9_]+)?!?\$?[A-Za-z]+\$?\d+:\$?[A-Za-z]+\$?\d+$/.test(typed));
          if (looksLikeRange) {
            wasm.session_set_list_validation_range(state.sheet, s.r0, s.c0, s.r1, s.c1, typed);
          } else {
            const vals = typed.split(",").map((x) => x.trim()).filter(Boolean);
            wasm.session_set_list_validation(state.sheet, s.r0, s.c0, s.r1, s.c1, vals);
          }
        } else {
          wasm.session_set_validation(
            state.sheet, s.r0, s.c0, s.r1, s.c1,
            kindSel.value, opSel.value, f1.value, f2.value, blank.checked, msg.value);
        }
        // Wording and the dropdown flag are a second write over the rule just
        // created, so the list path gets them too — it takes no message
        // arguments of its own.
        wasm.session_set_validation_messages(
          state.sheet, s.r0, s.c0, s.r1, s.c1, styleSel.value,
          [errTitle.value, msg.value, promptTitle.value, promptText.value],
          hideDrop.checked);
      } catch (e) { statusError(errText(e)); }
      draw();
    },
    "Remove",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_validation(state.sheet, s.r0, s.c0, s.r1, s.c1); }
      catch (e) { statusError(errText(e)); }
      draw();
    }
  );
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") apply.click(); });
  setTimeout(() => inp.focus(), 0);
}

export function buildCfPanel(body) {
  panelRangeReadout(body);
  panelLabel(body, "Highlight cells where the value…");
  const op = el("select", "panel-select");
  [["gt", "is greater than"], ["lt", "is less than"], ["eq", "equals"], ["between", "is between"],
   ["contains", "text contains"],
   // Decided from the whole range rather than the cell alone.
   ["top", "is in the top N"], ["bottom", "is in the bottom N"],
   ["toppct", "is in the top N%"], ["bottompct", "is in the bottom N%"],
   ["above", "is above average"], ["below", "is below average"],
   ["duplicate", "is duplicated"], ["unique", "appears only once"],
   ["colorscale", "— colour scale (2 stops)"],
   ["colorscale3", "— colour scale (3 stops)"], ["databar", "— data bar"]]
    .forEach(([v, t]) => { const o = el("option", null, t); o.value = v; op.appendChild(o); });
  body.appendChild(op);
  const a = el("input", "panel-field"); a.placeholder = "value"; a.spellcheck = false;
  const b = el("input", "panel-field"); b.placeholder = "and"; b.spellcheck = false; b.style.display = "none";
  body.appendChild(a); body.appendChild(b);
  // The scale/bar kinds are range-relative: they take no operand, and their
  // colours come from the swatch row rather than a single fill.
  const rangeRelative = () => op.value.startsWith("colorscale") || op.value === "databar";
  // Kinds needing a rank, and kinds needing no operand at all.
  const ranked = () => ["top", "bottom", "toppct", "bottompct"].includes(op.value);
  const noOperand = () => rangeRelative() || ["above", "below", "duplicate", "unique"].includes(op.value);
  op.addEventListener("change", () => {
    b.style.display = op.value === "between" ? "" : "none";
    a.style.display = noOperand() ? "none" : "";
    a.placeholder = op.value === "contains" ? "text" : ranked() ? "how many" : "value";
    panelHint.textContent = rangeRelative()
      ? "Colour comes from the value's position between the range's smallest and largest."
      : ranked() || noOperand()
        ? "Compared against the whole range, so adding rows can change which cells match."
        : "";
  });
  const panelHint = el("div", "panel-hint");
  body.appendChild(panelHint);
  panelLabel(body, "Fill color");
  const strip = el("div", "panel-swatches");
  let fill = "ffd166";
  ["ffd166", "d1f0d6", "ffd6e0", "d6e4ff", "fed7aa", "e9d5ff", "fca5a5", "a7f3d0"].forEach((hx, i) => {
    const sw = el("button", "swatch" + (i === 0 ? " on" : ""));
    sw.style.background = "#" + hx;
    sw.title = "#" + hx;
    sw.addEventListener("click", () => { fill = hx; strip.querySelectorAll(".swatch").forEach((x) => x.classList.remove("on")); sw.classList.add("on"); });
    strip.appendChild(sw);
  });
  body.appendChild(strip);
  panelActions(
    body,
    "Apply",
    () => {
      const s = effectiveRange();
      let kind = op.value;
      const av = parseFloat(a.value) || 0, bv = parseFloat(b.value) || 0;
      // Scale and bar colours travel in the text slot: a scale needs two or
      // three, which the single fill slot cannot carry.
      let txt = kind === "contains" ? a.value : "";
      // A ranked rule's operand is a count, and it defaults to the top 10 —
      // Excel's own default, and the one the rule type is named after.
      const ranks = ["top", "bottom", "toppct", "bottompct"];
      const rank = ranks.includes(kind) ? Math.max(1, parseInt(a.value, 10) || 10) : av;
      if (kind === "colorscale") { txt = `${fill},ffffff`; }
      else if (kind === "colorscale3") { kind = "colorscale"; txt = `${fill},ffffff,63be7b`; }
      else if (kind === "databar") { txt = fill; }
      try { wasm.session_add_cf(state.sheet, s.r0, s.c0, s.r1, s.c1, kind, rank, bv, txt, fill); }
      catch (e) { statusError(errText(e)); }
      draw();
    },
    "Clear",
    () => {
      const s = effectiveRange();
      try { wasm.session_clear_cf(state.sheet, s.r0, s.c0, s.r1, s.c1); }
      catch (e) { statusError(errText(e)); }
      draw();
    }
  );
  setTimeout(() => a.focus(), 0);
}

export function hyperlinkDialog() {
  const { row, col } = state.sel;
  let existing = null;
  try { existing = JSON.parse(wasm.session_hyperlink_at(state.sheet, row, col)); } catch {}
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent =
    existing ? `Edit link on ${A1(row, col)}` : `Insert link on ${A1(row, col)}`;
  body.textContent = "";

  const field = (label, value, placeholder) => {
    body.appendChild(el("div", "panel-label", label));
    const input = el("input", "panel-field");
    input.type = "text";
    input.value = value || "";
    input.placeholder = placeholder;
    body.appendChild(input);
    return input;
  };
  const target = field("Web address", existing && existing.target, "https://example.com");
  const location = field("Place in this workbook", existing && existing.location, "Sheet2!A1");
  const display = field("Text to display", existing && existing.display, "leave empty to keep the cell's own text");
  const tooltip = field("Tooltip", existing && existing.tooltip, "shown on hover");

  const row2 = el("div", "oc-confirm-actions");
  const commit = (clear) => {
    try {
      wasm.session_set_hyperlink(
        state.sheet, row, col,
        clear ? "" : target.value,
        clear ? "" : location.value,
        clear ? "" : tooltip.value,
        clear ? "" : display.value,
      );
      status.textContent = clear ? "link removed" : "link set";
    } catch (e) { statusError(errText(e)); }
    modal.hidden = true;
    draw();
  };
  if (existing) {
    const remove = el("button", "danger", "Remove link");
    remove.addEventListener("click", () => commit(true));
    row2.appendChild(remove);
  }
  const cancel = el("button", null, "Cancel");
  cancel.addEventListener("click", () => { modal.hidden = true; });
  const ok = el("button", "primary", existing ? "Update" : "Insert");
  ok.addEventListener("click", () => commit(false));
  row2.appendChild(cancel);
  row2.appendChild(ok);
  body.appendChild(row2);
  modal.hidden = false;
  setTimeout(() => target.focus(), 0);
}

export async function tableDialog() {
  let existing = null;
  try {
    existing = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col));
  } catch {}
  if (existing) { openPanel("table"); return; }

  const r = effectiveRange();
  // A single cell means "the block around it", as Ctrl+T does — asking someone
  // to select the whole table first is work the app can do.
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch {}
  }
  try {
    const name = wasm.session_create_table(
      state.sheet, bounds.r0, bounds.c0, bounds.r1, bounds.c1, "", true);
    select(bounds.r0, bounds.c0);
    status.textContent = `created ${name} — Esc closes the panel`;
  } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  openPanel("table");
}

export function confirmModal(title, message, confirmLabel = "OK") {
  return new Promise((resolve) => {
    const modal = byId("oc-modal");
    const body = byId("oc-modal-body");
    byId("oc-modal-title").textContent = title;
    body.textContent = "";
    const p = document.createElement("p");
    p.className = "oc-confirm-text";
    p.textContent = message;
    const row = document.createElement("div");
    row.className = "oc-confirm-actions";
    const cancel = document.createElement("button");
    cancel.className = "oc-btn";
    cancel.textContent = "Cancel";
    const ok = document.createElement("button");
    ok.className = "oc-btn primary";
    ok.textContent = confirmLabel;
    row.append(cancel, ok);
    body.append(p, row);
    modal.hidden = false;
    // The modal's own ✕ / backdrop wiring just hides it, which would leave this
    // promise pending and its key handler installed forever — so treat those
    // dismissals as "no" here too.
    const x = byId("oc-modal-x");
    const done = (answer) => {
      modal.hidden = true;
      body.textContent = "";
      document.removeEventListener("keydown", onKey, true);
      x.removeEventListener("click", onDismiss);
      modal.removeEventListener("click", onBackdrop);
      resolve(answer);
    };
    const onKey = (e) => {
      if (e.key === "Escape") { e.stopPropagation(); done(false); }
      else if (e.key === "Enter") { e.stopPropagation(); done(true); }
    };
    const onDismiss = () => done(false);
    const onBackdrop = (e) => { if (e.target === modal) done(false); };
    document.addEventListener("keydown", onKey, true);
    x.addEventListener("click", onDismiss);
    modal.addEventListener("click", onBackdrop);
    cancel.addEventListener("click", () => done(false));
    ok.addEventListener("click", () => done(true));
    ok.focus();
  });
}

export function reportImportIssues() {
  let summary = "";
  try { summary = wasm.session_import_summary(); } catch {}
  if (!summary) return;
  const bar = byId("tb-status");
  // The summary names parts of the file that did not survive import, so it
  // quotes the workbook — sheet names, defined names, function names. Re-parsing
  // `bar.textContent` as markup made that a second injection point on top of
  // the first.
  const warn = document.createElement("span");
  warn.className = "warn";
  warn.textContent = summary;
  bar.replaceChildren(document.createTextNode(`${bar.textContent} — `), warn);
}

export function pasteSpecialDialog() {
  if (!wasm.session_clip_has()) { status.textContent = "clipboard is empty"; return; }
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Paste special";
  body.textContent = "";

  let what = "all";
  const group = (label, options, onPick) => {
    body.append(el("p", "oc-confirm-text", label));
    const row = el("div", "fc-row");
    options.forEach(([v, t], i) => {
      const l = el("label", "fc-check");
      const r = document.createElement("input");
      r.type = "radio";
      r.name = "ps-" + label;
      r.value = v;
      if (i === 0) r.checked = true;
      r.addEventListener("change", () => onPick(v));
      l.append(r, document.createTextNode(" " + t));
      row.appendChild(l);
    });
    body.appendChild(row);
  };
  group("Paste", [
    ["all", "Everything"], ["values", "Values only"],
    ["formulas", "Formulas"], ["formats", "Formats only"],
    // Excel has this as its own option for the reason a plain paste must not
    // have it: pasting three cells should not reshape the sheet they land in.
    // Asked for deliberately, it is exactly what somebody rebuilding a layout
    // wants — and it is what a cross-application paste cannot carry, because
    // the clipboard's HTML says nothing this engine will act on (docs/68).
    ["widths", "Column widths only"],
  ], (v) => { what = v; });

  let op = "none";
  group("Operation", [
    ["none", "None"], ["add", "Add"], ["subtract", "Subtract"],
    ["multiply", "Multiply"], ["divide", "Divide"],
  ], (v) => { op = v; });

  const tWrap = el("label", "fc-check");
  const transpose = document.createElement("input");
  transpose.type = "checkbox";
  tWrap.append(transpose, document.createTextNode(" Transpose"));
  body.append(tWrap);
  body.append(el("div", "panel-hint",
    "An operation combines the copied numbers with what is already there. Non-numeric cells are left alone."));

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Paste");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    close();
    canvas.focus();
    // An arithmetic operation is what to *do*, so it wins over what to paste;
    // transpose is a placement and only applies to a plain paste.
    const mode = op !== "none" ? op : transpose.checked ? "transpose" : what;
    doPasteMode(mode);
  });
  ok.focus();
}

export function textToColumnsDialog() {
  const s0 = effectiveRange();
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Text to columns";
  body.textContent = "";
  body.append(el("p", "oc-confirm-text",
    `Split column ${colName(s0.c0)}, rows ${s0.r0 + 1}–${s0.r1 + 1}, into the columns to its right.`));

  let delim = ",";
  const row = el("div", "fc-row");
  for (const [v, t] of [[",", "Comma"], ["\t", "Tab"], [";", "Semicolon"], [" ", "Space"], ["", "Custom"]]) {
    const l = el("label", "fc-check");
    const r = document.createElement("input");
    r.type = "radio"; r.name = "ttc"; r.value = v;
    if (v === ",") r.checked = true;
    r.addEventListener("change", () => { delim = v === "" ? custom.value : v; });
    l.append(r, document.createTextNode(" " + t));
    row.appendChild(l);
  }
  const custom = el("input", "panel-field");
  custom.placeholder = "delimiter";
  custom.style.maxWidth = "120px";
  custom.addEventListener("input", () => {
    const c = row.querySelector('input[value=""]');
    if (c) { c.checked = true; delim = custom.value; }
  });
  body.append(row, custom);
  const warn = el("div", "panel-hint", "");
  body.append(warn);

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Split");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    if (!delim) { warn.textContent = "Choose a delimiter first."; return; }
    close();
    canvas.focus();
    tryEdit(() => {
      let widest = 0;
      for (let r = s0.r0; r <= s0.r1; r++) {
        const text = wasm.session_cell_input(state.sheet, r, s0.c0);
        // Only literal text splits; a formula's result is not the user's text,
        // and overwriting the formula would lose it.
        if (!text || text.startsWith("=")) continue;
        const parts = text.split(delim);
        widest = Math.max(widest, parts.length);
        parts.forEach((part, i) => {
          wasm.session_set_cell(state.sheet, r, s0.c0 + i, part.trim());
        });
      }
      status.textContent = widest > 1
        ? `split into ${widest} columns`
        : "nothing to split — no cell contained the delimiter";
    });
  });
  ok.focus();
}

export function openNameBoxList() {
  closeSheetMenu();
  let names = [];
  try { names = JSON.parse(wasm.session_names()); } catch {}
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  if (!names.length) {
    menu.appendChild(el("div", "panel-hint", "No defined names yet."));
  } else {
    for (const n of names) {
      const b = el("button", "menu-item", n.name || n);
      b.addEventListener("click", () => { closeSheetMenu(); gotoName(n.name || n); canvas.focus(); });
      menu.appendChild(b);
    }
  }
  const r = cellRef.getBoundingClientRect();
  positionMenu(menu, r.left, r.bottom + 2);
}

export function openNameManager(x, y) {
  closeSheetMenu();
  let names = [];
  try { names = JSON.parse(wasm.session_names()); } catch {}
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu nm-menu";
  menu.id = "sheet-ctx";
  const head = document.createElement("div");
  head.className = "menu-label";
  head.textContent = names.length ? "Named ranges" : "No named ranges yet";
  menu.appendChild(head);
  names.forEach((n) => {
    const row = document.createElement("div");
    row.className = "nm-row";
    const go = document.createElement("button");
    go.className = "nm-go";
    // Built, not interpolated. Both of these are workbook text: a `refersTo`
    // reading `<img src=x onerror=...>` used to become a real element here, and
    // opening the Name Manager on a file somebody sent you ran their script in
    // this origin. Elements and `textContent` cannot do that.
    const label = document.createElement("b");
    label.textContent = n.name;
    const target = document.createElement("span");
    target.textContent = n.refersTo;
    go.replaceChildren(label, target);
    go.addEventListener("click", () => { closeSheetMenu(); gotoName(n.name); });
    const del = document.createElement("button");
    del.className = "nm-del";
    del.textContent = "×";
    del.title = "Delete";
    // The row only goes when the name did. Removing it regardless took the
    // entry off the list while the workbook still held it, so the next time the
    // menu was opened it was back and nobody knew why.
    del.addEventListener("click", (e) => {
      e.stopPropagation();
      try { wasm.session_delete_name(n.name); }
      catch (err) { statusError(errText(err)); return; }
      row.remove();
      draw();
    });
    row.appendChild(go); row.appendChild(del);
    menu.appendChild(row);
  });
  positionMenu(menu, x, y);
}

export function closeSheetMenu() {
  const m = byId("sheet-ctx");
  if (m) m.remove();
}

export function sheetMenu(i, x, y) {
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  const item = (label, danger, fn) => {
    const btn = document.createElement("button");
    btn.textContent = label;
    if (danger) btn.className = "danger";
    btn.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(btn);
  };
  item("Rename", false, () => renameSheet(i, tabsEl.querySelectorAll(".sheet-tab")[i]));
  item("Duplicate", false, () => {
    try { const n = wasm.session_duplicate_sheet(i); switchSheet(n); renderTabs(); }
    catch (e) { statusError(errText(e)); }
  });
  // Tab-color swatch strip.
  const sep = document.createElement("div");
  sep.className = "menu-sep";
  menu.appendChild(sep);
  const lbl = document.createElement("div");
  lbl.className = "menu-label";
  lbl.textContent = "Tab color";
  menu.appendChild(lbl);
  const strip = document.createElement("div");
  strip.className = "swatch-row";
  const setTabColor = (hex) => {
    closeSheetMenu();
    try { wasm.session_set_tab_color(i, hex); renderTabs(); }
    catch (e) { statusError(errText(e)); }
  };
  ["E53935", "FB8C00", "FDD835", "43A047", "1E88E5", "5E35B1", "8E24AA", "546E7A"].forEach((hex) => {
    const sw = document.createElement("button");
    sw.className = "swatch";
    sw.style.background = "#" + hex;
    sw.title = "#" + hex;
    sw.addEventListener("click", (e) => { e.stopPropagation(); setTabColor(hex); });
    strip.appendChild(sw);
  });
  const none = document.createElement("button");
  none.className = "swatch swatch-none";
  none.title = "No color";
  none.addEventListener("click", (e) => { e.stopPropagation(); setTabColor(""); });
  strip.appendChild(none);
  menu.appendChild(strip);
  menu.appendChild(document.createElement("div")).className = "menu-sep";
  let prot = [];
  try { prot = JSON.parse(wasm.session_sheet_protected()); } catch {}
  item(prot[i] ? "Unprotect sheet" : "Protect sheet", false, () => {
    try { wasm.session_set_sheet_protected(i, !prot[i]); renderTabs(); draw(); }
    catch (e) { statusError(errText(e)); }
    status.textContent = prot[i] ? "sheet unprotected" : "sheet protected";
  });
  item("Hide sheet", false, () => {
    try { wasm.session_set_sheet_visibility(i, "hidden"); renderTabs(); draw(); }
    catch (e) { status.textContent = `${e}`.replace(/^Error:\s*/, ""); }
  });
  // Unhide lists the hidden sheets by name, since they have no tab to click.
  let vis = [];
  try { vis = JSON.parse(wasm.session_sheet_visibility()); } catch {}
  const names = JSON.parse(wasm.session_sheet_names());
  // `veryHidden` is deliberately absent: Excel does not offer it here either,
  // and silently promoting it to merely hidden would undo the author's choice.
  const hidden = names
    .map((n, idx) => ({ n, idx }))
    .filter(({ idx }) => vis[idx] === "hidden");
  if (hidden.length) {
    for (const { n, idx } of hidden) {
      item(`Unhide "${n}"`, false, () => {
        try { wasm.session_set_sheet_visibility(idx, "visible"); renderTabs(); draw(); }
        catch (e) { status.textContent = `${e}`; }
      });
    }
    menu.appendChild(document.createElement("div")).className = "menu-sep";
  }
  item("Delete", true, () => {
    try {
      wasm.session_delete_sheet(i);
      if (i <= state.sheet) state.sheet = Math.max(0, state.sheet - 1);
      renderTabs();
      resetView();
    } catch (e) { statusError(errText(e)); }
  });
  positionMenu(menu, x, y);
}

export function positionMenu(menu, x, y) {
  menu.style.left = "0px";
  menu.style.top = "0px";
  menu.style.visibility = "hidden";
  ocOverlayHost.appendChild(menu);
  const h = menu.offsetHeight, w = menu.offsetWidth;
  menu.style.top = (y + h > window.innerHeight ? Math.max(4, y - h) : y) + "px";
  menu.style.left = (x + w > window.innerWidth ? Math.max(4, x - w) : x) + "px";
  menu.style.visibility = "visible";
  setTimeout(() => document.addEventListener("click", closeSheetMenu, { once: true }), 0);
}

export function refreshValidationPrompt() {
  const box = byId("dv-prompt");
  if (!box) return;
  let hint = "";
  try {
    hint = wasm ? wasm.session_validation_prompt(state.sheet, state.sel.row, state.sel.col) : "";
  } catch {}
  if (!hint || state.selKind !== "cells") { box.hidden = true; return; }
  let p;
  try { p = JSON.parse(hint); } catch { box.hidden = true; return; }
  box.textContent = "";
  if (p.title) box.appendChild(el("strong", null, p.title));
  if (p.text) box.appendChild(el("span", null, p.text));
  const x = colXAt(state.sel.col), y = rowYAt(state.sel.row);
  if (x === undefined || y === undefined) { box.hidden = true; return; }
  const rect = canvas.getBoundingClientRect();
  box.style.left = `${rect.left + x}px`;
  box.style.top = `${rect.top + y + rowHAt(state.sel.row) + 4}px`;
  box.hidden = false;
}

export function cellMenu(x, y) {
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu";
  menu.id = "sheet-ctx";
  const hideSubs = () => menu.querySelectorAll(".ctx-submenu").forEach((s) => (s.hidden = true));
  const sep = () => menu.appendChild(el("div", "menu-sep"));
  const item = (label, danger, fn) => {
    const b = el("button", danger ? "danger" : null, label);
    b.addEventListener("mouseenter", hideSubs);
    b.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(b);
  };
  // A submenu row (label + ›); its child popmenu is nested so it is removed
  // with the parent, and fixed-positioned to the parent row's right edge.
  const submenu = (label, entries) => {
    const b = el("button", "has-sub", label);
    b.setAttribute("aria-haspopup", "true");
    const sub = el("div", "popmenu ctx-submenu");
    sub.hidden = true;
    for (const [lbl, danger, fn] of entries) {
      const c = el("button", danger ? "danger" : null, lbl);
      c.addEventListener("click", (e) => { e.stopPropagation(); closeSheetMenu(); fn(); });
      sub.appendChild(c);
    }
    const openSub = () => {
      hideSubs();
      sub.hidden = false;
      const r = b.getBoundingClientRect();
      const sw = sub.offsetWidth, sh = sub.offsetHeight;
      let left = r.right - 2;
      if (left + sw > window.innerWidth - 4) left = Math.max(4, r.left - sw + 2);
      let top = r.top - 4;
      if (top + sh > window.innerHeight - 4) top = Math.max(4, window.innerHeight - 4 - sh);
      sub.style.left = left + "px";
      sub.style.top = top + "px";
    };
    b.addEventListener("mouseenter", openSub);
    b.addEventListener("click", (e) => { e.stopPropagation(); sub.hidden ? openSub() : (sub.hidden = true); });
    menu.appendChild(b);
    menu.appendChild(sub);
  };
  const span = () => { const r = effectiveRange(); return { r, rows: r.r1 - r.r0 + 1, cols: r.c1 - r.c0 + 1 }; };

  item("Cut", false, () => doCut());
  item("Copy", false, () => doCopy());
  item("Paste", false, () => doPaste());
  submenu("Paste special", [
    ["Paste special…", false, () => pasteSpecialDialog()],
    ["Values only", false, () => doPasteMode("values")],
    ["Formulas only", false, () => doPasteMode("formulas")],
    ["Formats only", false, () => doPasteMode("formats")],
    ["Transpose", false, () => doPasteMode("transpose")],
  ]);
  sep();
  // Insert/delete *cells*, shifting the rest. References are not rewritten, so
  // the user is told when that matters rather than discovering it later.
  const shiftCells = (insert, vertical, label) => () => {
    const r = effectiveRange();
    const risky = shiftIsRisky(() =>
      wasm.session_shift_affects_formulas(state.sheet, r.r0, r.c0, r.r1, r.c1, vertical));
    const run = () => {
      tryEdit(() => wasm.session_shift_cells(state.sheet, r.r0, r.c0, r.r1, r.c1, insert, vertical));
      status.textContent = label.toLowerCase();
    };
    if (!risky) { run(); return; }
    confirmModal(
      "Formulas reference these cells",
      "Moving them will not adjust those references — they will keep pointing at the same addresses, which will now hold different cells.",
      label,
    ).then((ok) => { if (ok) run(); });
  };
  submenu("Insert cells", [
    ["Shift cells down", false, shiftCells(true, true, "Inserted, shifted down")],
    ["Shift cells right", false, shiftCells(true, false, "Inserted, shifted right")],
  ]);
  submenu("Delete cells", [
    ["Shift cells up", false, shiftCells(false, true, "Deleted, shifted up")],
    ["Shift cells left", false, shiftCells(false, false, "Deleted, shifted left")],
  ]);
  submenu("Insert", [
    ["Row above", false, () => { const { r, rows } = span(); tryEdit(() => wasm.session_insert_rows(state.sheet, r.r0, rows)); }],
    ["Row below", false, () => { const { r, rows } = span(); tryEdit(() => wasm.session_insert_rows(state.sheet, r.r1 + 1, rows)); }],
    ["Column left", false, () => { const { r, cols } = span(); tryEdit(() => wasm.session_insert_columns(state.sheet, r.c0, cols)); }],
    ["Column right", false, () => { const { r, cols } = span(); tryEdit(() => wasm.session_insert_columns(state.sheet, r.c1 + 1, cols)); }],
  ]);
  submenu("Delete", [
    ["Row", true, () => { const { r, rows } = span(); tryEdit(() => wasm.session_delete_rows(state.sheet, r.r0, rows)); }],
    ["Column", true, () => { const { r, cols } = span(); tryEdit(() => wasm.session_delete_columns(state.sheet, r.c0, cols)); }],
  ]);
  submenu("Hide", [
    ["Row", false, () => { const { r } = span(); tryEdit(() => wasm.session_hide_rows(state.sheet, r.r0, r.r1)); }],
    ["Column", false, () => { const { r } = span(); tryEdit(() => wasm.session_hide_cols(state.sheet, r.c0, r.c1)); }],
    ["Unhide rows/cols", false, () => { const { r } = span(); tryEdit(() => { wasm.session_unhide_rows(state.sheet, r.r0, r.r1); wasm.session_unhide_cols(state.sheet, r.c0, r.c1); }); }],
  ]);
  sep();
  submenu("Clear", [
    ["Contents", false, () => clearSelection()],
    ["Formats", false, () => clearFormats()],
    ["All (incl. formats)", true, () => clearAll()],
  ]);
  submenu("Sort", [
    [`${colName(state.sel.col)} A → Z`, false, () => sortRange(false)],
    [`${colName(state.sel.col)} Z → A`, false, () => sortRange(true)],
    ["Custom sort…", false, () => sortDialog()],
  ]);
  sep();
  // The things you reach for *from a cell* — previously only on the toolbar or
  // the menu bar, which is a long way to go for something the right-click is
  // already asking about.
  item("Format cells…", false, () => formatCellsDialog());
  // The verb reflects what is actually there, so the menu is not offering to
  // insert a comment onto a cell that already has a thread.
  item(
    readThread(state.sel.row, state.sel.col) ? "Show comments" : "Insert comment",
    false,
    () => { if (activePanel !== "note") togglePanel("note"); else panelNote?.refresh(); },
  );
  item(
    (() => {
      let has = null;
      try { has = JSON.parse(wasm.session_hyperlink_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
      return has ? "Edit link…" : "Insert link…";
    })(),
    false,
    () => hyperlinkDialog(),
  );
  item(
    (() => {
      let t = null;
      try { t = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
      return t ? "Convert to range…" : "Create table…";
    })(),
    false,
    () => tableDialog(),
  );
  (() => {
    let t = null;
    try { t = JSON.parse(wasm.session_table_at(state.sheet, state.sel.row, state.sel.col)); } catch {}
    if (!t) return;
    item(t.totals > 0 ? "Hide totals row" : "Show totals row", false, () => {
      try {
        wasm.session_table_totals(state.sheet, state.sel.row, state.sel.col, t.totals === 0);
        status.textContent = t.totals > 0 ? "totals row hidden" : "totals row shown";
      } catch (e) { statusError(errText(e)); }
      draw();
    });
  })();
  item("Define name…", false, () => {
    const r = canvas.getBoundingClientRect();
    openNameManager(r.left + 120, r.top + 90);
  });
  item("Filter", false, () => toggleFilter());
  positionMenu(menu, x, y);
}

export function sizeDialog(axis, index) {
  const isCol = axis === "col";
  let current = 0;
  try {
    current = isCol
      ? JSON.parse(wasm.session_col_px(state.sheet, index, 1))[0]
      : JSON.parse(wasm.session_row_px(state.sheet, index, 1))[0];
  } catch {}
  const label = isCol ? `Column ${colName(index)} width (px)` : `Row ${index + 1} height (px)`;
  const answer = window.prompt(label, String(current || (isCol ? COL_W : ROW_H)));
  if (answer === null) return;
  const px = Math.round(parseFloat(answer));
  if (!Number.isFinite(px) || px < 0) { status.textContent = "not a size"; return; }
  const r = selRect();
  tryEdit(() => {
    if (isCol) for (let c = r.c0; c <= r.c1; c += 1) wasm.session_set_col_width(state.sheet, c, px);
    else for (let row = r.r0; row <= r.r1; row += 1) wasm.session_set_row_height(state.sheet, row, px);
  });
}

export function headerMenu(axis, x, y) {
  closeSheetMenu();
  const isCol = axis === "col";
  const menu = el("div", "popmenu ctx-menu");
  menu.id = "sheet-ctx";
  const item = (label, danger, fn) => {
    const b = el("button", danger ? "danger" : null, label);
    b.addEventListener("click", () => { closeSheetMenu(); fn(); });
    menu.appendChild(b);
  };
  const sep = () => menu.appendChild(el("div", "menu-sep"));
  const span = () => {
    const r = effectiveRange();
    return { r, n: isCol ? r.c1 - r.c0 + 1 : r.r1 - r.r0 + 1 };
  };
  const what = isCol ? "column" : "row";
  const plural = (n) => (n === 1 ? what : `${what}s`);

  item("Cut", false, () => doCut());
  item("Copy", false, () => doCopy());
  item("Paste", false, () => doPaste());
  sep();
  const { n } = span();
  item(isCol ? `Insert ${n} ${plural(n)} left` : `Insert ${n} ${plural(n)} above`, false, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_insert_columns(state.sheet, r.c0, count)
      : wasm.session_insert_rows(state.sheet, r.r0, count)));
  });
  item(isCol ? `Insert ${n} ${plural(n)} right` : `Insert ${n} ${plural(n)} below`, false, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_insert_columns(state.sheet, r.c1 + 1, count)
      : wasm.session_insert_rows(state.sheet, r.r1 + 1, count)));
  });
  item(`Delete ${n} ${plural(n)}`, true, () => {
    const { r, n: count } = span();
    tryEdit(() => (isCol
      ? wasm.session_delete_columns(state.sheet, r.c0, count)
      : wasm.session_delete_rows(state.sheet, r.r0, count)));
  });
  item("Clear contents", false, () => clearSelection());
  sep();
  item(isCol ? "Column width…" : "Row height…", false, () => {
    const r = selRect();
    sizeDialog(axis, isCol ? r.c0 : r.r0);
  });
  item(isCol ? "Autofit width" : "Autofit height", false, () => {
    const r = selRect();
    if (isCol) for (let c = r.c0; c <= r.c1; c += 1) autofitColumn(c);
    else for (let row = r.r0; row <= r.r1; row += 1) autofitRow(row);
  });
  sep();
  item(`Hide ${plural(n)}`, false, () => {
    const { r } = span();
    tryEdit(() => (isCol
      ? wasm.session_hide_cols(state.sheet, r.c0, r.c1)
      : wasm.session_hide_rows(state.sheet, r.r0, r.r1)));
  });
  item("Unhide", false, () => {
    const { r } = span();
    tryEdit(() => (isCol
      ? wasm.session_unhide_cols(state.sheet, r.c0, r.c1)
      : wasm.session_unhide_rows(state.sheet, r.r0, r.r1)));
  });
  positionMenu(menu, x, y);
}

export function openColumnFilterForTest(col) {
  openColumnFilter(col, 100, 100);
}

/// File → Properties: what the document says about itself.
///
/// `DocumentProperties` has been in the model, imported from `docProps/core.xml`
/// and written back to it, since long before this dialog existed — nine fields
/// round-tripping faithfully with no way for the person editing the file to see
/// one of them. A workbook opened here kept its title and author perfectly and
/// showed neither; one created here went out with none (`UX-META-01`).
///
/// Five fields are editable and four are shown read-only. `created`, `modified`
/// and `lastModifiedBy` are the file's own history rather than opinions about
/// it, and offering them as text boxes would invite writing a false history into
/// a document. `language` belongs to the content.
///
/// The read-only half is shown rather than hidden because that is the half an
/// enterprise reader actually came for: who touched this, and when.
export function documentPropertiesDialog() {
  let props = {};
  try {
    props = JSON.parse(wasm.session_doc_properties());
  } catch {
    statusError("could not read the document properties");
    return;
  }

  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Document properties";
  body.textContent = "";

  const form = el("div", "oc-props");
  const fields = [];
  const field = (key, label, hint) => {
    const wrap = el("label", "oc-props-row");
    wrap.append(el("span", "oc-props-label", label));
    const input = document.createElement("input");
    input.type = "text";
    input.className = "oc-input";
    input.value = props[key] ?? "";
    if (hint) input.placeholder = hint;
    wrap.append(input);
    form.append(wrap);
    fields.push([key, input]);
    return input;
  };

  const first = field("title", "Title");
  field("subject", "Subject");
  field("description", "Description");
  field("keywords", "Keywords", "comma separated");
  field("creator", "Author");

  // The file's own account of itself. Empty is stated rather than left blank:
  // "—" says the file carries nothing, where a gap looks like a bug.
  const facts = el("div", "oc-props-facts");
  for (const [label, value] of [
    ["Created", props.created],
    ["Modified", props.modified],
    ["Last saved by", props.lastModifiedBy],
    ["Language", props.language],
  ]) {
    const row = el("div", "oc-props-row");
    row.append(el("span", "oc-props-label", label));
    row.append(el("span", "oc-props-fact", value && value.trim() ? value : "—"));
    facts.append(row);
  }

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const save = el("button", "oc-btn primary", "Save");
  actions.append(cancel, save);
  body.append(form, facts, actions);
  modal.hidden = false;

  const x = byId("oc-modal-x");
  const close = () => {
    modal.hidden = true;
    body.textContent = "";
    document.removeEventListener("keydown", onKey, true);
    x.removeEventListener("click", close);
    modal.removeEventListener("click", onBackdrop);
    canvas.focus();
  };
  const commitProps = () => {
    const v = Object.fromEntries(fields.map(([k, i]) => [k, i.value]));
    try {
      wasm.session_set_doc_properties(v.title, v.subject, v.description, v.keywords, v.creator);
      status.textContent = "properties saved";
    } catch (e) {
      statusError(errText(e));
    }
    close();
  };
  const onKey = (e) => {
    if (e.key === "Escape") { e.stopPropagation(); close(); }
    // Enter saves from any field, which is what a form this small should do.
    else if (e.key === "Enter") { e.stopPropagation(); commitProps(); }
  };
  const onBackdrop = (e) => { if (e.target === modal) close(); };
  document.addEventListener("keydown", onKey, true);
  x.addEventListener("click", close);
  modal.addEventListener("click", onBackdrop);
  cancel.addEventListener("click", close);
  save.addEventListener("click", commitProps);
  first.focus();
  first.select();
}
