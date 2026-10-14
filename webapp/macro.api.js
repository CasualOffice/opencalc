// The ONLYOFFICE-compatible Office API a macro sees (`MACRO-01`, docs/99 §2).
//
// A macro written for ONLYOFFICE's spreadsheet editor runs here unchanged, so
// the names, argument order, return types and indexing below follow ONLYOFFICE's
// published reference, not our taste:
//
//   https://api.onlyoffice.com/docs/office-api/usage-api/spreadsheet-api/
//   (source: github.com/ONLYOFFICE/api.onlyoffice.com,
//    site/docs/office-api/usage-api/spreadsheet-api/{Api,ApiWorksheet,ApiRange,
//    ApiColor,ApiFreezePanes}/Methods/*.md)
//
// The indexing is ONLYOFFICE's and it is mixed, so it is worth stating once:
//
//   * `ApiWorksheet.GetRangeByNumber(row, col)`, `SetColumnWidth(col, …)` and
//     `SetRowHeight(row, …)` are **0-based** (their samples address column A as
//     0 and walk rows from 0).
//   * `ApiRange.GetRow()` / `GetCol()` return **1-based** numbers, as a user
//     reads them off the headers.
//   * `ApiWorksheet.GetRows(n)`, `ApiRange.GetRows(n)` and `ApiRange.GetCols(n)`
//     are **1-based** ("starts counting from 1, the 0 value returns an error").
//   * `ApiWorksheet.GetIndex()` is 0-based.
//
// **This module holds no workbook.** It is handed `engine` — the wasm bindings
// of a private *copy* of the workbook living in the macro's worker — and
// `record`, which it calls with every mutating engine call it made. Each
// mutation goes to the copy first, so the macro's own later reads see it
// (recalculated, by the same engine), and only a call the copy accepted is
// recorded. The editor then replays the recorded calls against the real
// workbook as one undo step (`editor.macros.js`). Reads never leave the
// worker; writes leave it only as that list.
//
// **An unimplemented method throws, never no-ops.** Every object is wrapped in
// a proxy that answers any capitalised name it does not define with a function
// throwing `NotSupported: ApiRange.SetSort` — a macro that silently skipped a
// step would report success on a workbook it had not finished changing.

export const MAX_ROWS = 1048576;
export const MAX_COLS = 16384;

/// The most cells one call may touch. A whole-column format is 1,048,576
/// cells; the bound is there so `GetRange("A:XFD").SetBold(true)` is refused by
/// name rather than taking the tab down.
const MAX_CELLS_PER_CALL = 2_000_000;

export class NotSupported extends Error {
  constructor(what) {
    super(`NotSupported: ${what}`);
    this.name = "NotSupported";
  }
}

export class MacroError extends Error {
  constructor(name, message) {
    super(`${name}: ${message}`);
    this.name = name;
  }
}

// ---------------------------------------------------------------------------
// A1 references.

function colName(c) {
  let s = "";
  let n = c + 1;
  while (n > 0) {
    const m = (n - 1) % 26;
    s = String.fromCharCode(65 + m) + s;
    n = Math.floor((n - m) / 26);
  }
  return s;
}

function colIndex(letters) {
  let n = 0;
  for (const ch of letters.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64);
  return n - 1;
}

/// One side of a reference: `A1`, `$B$7`, `C` (a whole column) or `12` (a row).
function parsePart(s) {
  const m = /^\$?([A-Za-z]{1,3})?\$?(\d{1,7})?$/.exec(s.trim());
  if (!m || (!m[1] && !m[2])) return null;
  return {
    col: m[1] ? colIndex(m[1]) : null,
    row: m[2] ? Number(m[2]) - 1 : null,
  };
}

/// `"A1:B2"` (optionally `Sheet!`-qualified) to `{ sheetName, r0, c0, r1, c1 }`,
/// 0-based and inclusive. `null` when it is not a reference.
export function parseRef(text) {
  let s = String(text).trim();
  let sheetName = null;
  const bang = s.lastIndexOf("!");
  if (bang >= 0) {
    sheetName = s.slice(0, bang).replace(/^'(.*)'$/, "$1").replace(/''/g, "'");
    s = s.slice(bang + 1);
  }
  const [a, b, extra] = s.split(":");
  if (extra !== undefined) return null;
  const p = parsePart(a);
  const q = b === undefined ? p : parsePart(b);
  if (!p || !q) return null;
  // A part with only a column or only a row must be paired with its own kind:
  // `A:C` and `1:4` are references, `A:4` is not.
  if ((p.col === null) !== (q.col === null) || (p.row === null) !== (q.row === null)) return null;
  const c0 = p.col ?? 0, c1 = q.col ?? MAX_COLS - 1;
  const r0 = p.row ?? 0, r1 = q.row ?? MAX_ROWS - 1;
  if (r0 < 0 || c0 < 0 || r1 >= MAX_ROWS || c1 >= MAX_COLS) return null;
  return {
    sheetName,
    r0: Math.min(r0, r1), c0: Math.min(c0, c1),
    r1: Math.max(r0, r1), c1: Math.max(c0, c1),
  };
}

// ---------------------------------------------------------------------------

/// Answer every capitalised name the class does not define with a
/// `NotSupported` thrower. Lower-case names (`then`, `toString`, `toJSON`) fall
/// through to ordinary JavaScript, so `await range` and string coercion behave.
function guarded(obj, className) {
  return new Proxy(obj, {
    get(target, prop, receiver) {
      if (prop in target) return Reflect.get(target, prop, receiver);
      if (typeof prop === "string" && /^[A-Z]/.test(prop)) {
        return function notSupported() { throw new NotSupported(`${className}.${prop}`); };
      }
      return undefined;
    },
  });
}

const intArg = (v, what) => {
  const n = Number(v);
  if (!Number.isInteger(n)) throw new MacroError("TypeError", `${what} must be an integer, got ${String(v)}`);
  return n;
};

/// How a JavaScript value is typed into a cell: what `SetValue` hands to the
/// engine's typed-entry path, the same one the formula bar uses.
function entryText(v) {
  if (v === null || v === undefined) return "";
  if (typeof v === "boolean") return v ? "TRUE" : "FALSE";
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw new MacroError("TypeError", `cannot write ${v} to a cell`);
    return String(v);
  }
  if (typeof v === "object" && v instanceof Date) {
    throw new NotSupported("ApiRange.SetValue(Date)");
  }
  return String(v);
}

/// Build the `Api` global for one run.
///
/// `ctx`: `{ engine, record(fn, args), ui(op), selection: {sheet, r0, c0, r1,
/// c1, row, col}, maxOps }`. `record` counts and throws `LimitExceeded` past
/// `maxOps`; this module only calls it.
export function createApi(ctx) {
  const E = ctx.engine;

  /// Call an engine mutation on the private copy, then record it. Order is the
  /// point: a call the copy refused (a protected sheet, a bad name) throws to
  /// the macro and is never recorded, so the replay cannot fail where the copy
  /// succeeded.
  function mutate(fn, ...args) {
    const out = E[fn](...args);
    ctx.record(fn, args);
    return out;
  }

  const sheetNames = () => JSON.parse(E.session_sheet_names());
  const usedBounds = (sheet) => JSON.parse(E.session_used_bounds(sheet));

  // Live worksheet handles, so a sheet deleted mid-macro shifts the indices of
  // the ones after it rather than leaving them pointing at a neighbour.
  const handles = [];
  function sheetHandle(index) {
    let h = handles.find((x) => x.index === index && !x.deleted);
    if (!h) {
      h = { index, deleted: false };
      handles.push(h);
    }
    return h;
  }

  // ---- ApiColor ------------------------------------------------------------

  class ApiColor {
    constructor(r, g, b) {
      this._r = r & 255; this._g = g & 255; this._b = b & 255;
    }
    GetClassType() { return "color"; }
    /// ONLYOFFICE answers a number: `(r << 16) | (g << 8) | b`.
    GetRGB() { return (this._r << 16) | (this._g << 8) | this._b; }
    GetHex() { return "#" + this._hex(); }
    _hex() {
      return [this._r, this._g, this._b].map((x) => x.toString(16).padStart(2, "0")).join("").toUpperCase();
    }
  }
  const newColor = (r, g, b) => guarded(new ApiColor(r, g, b), "ApiColor");
  const colorFromHex = (hex) => {
    const h = String(hex).replace(/^#/, "").replace(/^FF(?=[0-9a-f]{6}$)/i, "");
    if (!/^[0-9a-f]{6}$/i.test(h)) return null;
    const n = parseInt(h, 16);
    return newColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
  };
  const hexOf = (color, method) => {
    if (color === null || color === undefined || color === "No Fill") return "";
    if (typeof color === "object" && typeof color._hex === "function") return color._hex();
    throw new MacroError("TypeError", `${method} expects an ApiColor (Api.CreateColorFromRGB)`);
  };

  // ---- ApiRange ------------------------------------------------------------

  class ApiRange {
    /// `kind` is `"cells"`, `"rows"` or `"cols"` — what `GetRows()` and
    /// `GetCols()` hand back, which changes what `GetCount()` counts.
    constructor(sheetH, r0, c0, r1, c1, kind = "cells") {
      this._h = sheetH;
      this._r0 = r0; this._c0 = c0; this._r1 = r1; this._c1 = c1;
      this._kind = kind;
    }

    get _sheet() {
      if (this._h.deleted) throw new MacroError("InvalidReference", "the range's sheet was deleted");
      return this._h.index;
    }
    _cells() { return (this._r1 - this._r0 + 1) * (this._c1 - this._c0 + 1); }
    _bounded(method) {
      if (this._cells() > MAX_CELLS_PER_CALL) {
        throw new MacroError("LimitExceeded", `ApiRange.${method} on ${this._cells()} cells (limit ${MAX_CELLS_PER_CALL})`);
      }
    }
    /// The part of this range that can hold anything: the rest is empty by
    /// definition, so reads and clears stop at the used bounds.
    _used() {
      const b = usedBounds(this._sheet);
      const r1 = Math.min(this._r1, b.rows - 1), c1 = Math.min(this._c1, b.cols - 1);
      return { r0: this._r0, c0: this._c0, r1, c1, empty: r1 < this._r0 || c1 < this._c0 };
    }
    _style(fn, ...rest) {
      this._bounded(fn);
      mutate(fn, this._sheet, this._r0, this._c0, this._r1, this._c1, ...rest);
      return true;
    }
    _sub(r0, c0, r1, c1, kind = "cells") {
      return newRange(this._h, r0, c0, r1, c1, kind);
    }

    GetClassType() { return "range"; }

    // -- addressing

    GetRow() { return this._r0 + 1; }
    GetCol() { return this._c0 + 1; }
    GetAddress(rowAbs, colAbs) {
      const rd = rowAbs ? "$" : "", cd = colAbs ? "$" : "";
      const one = (r, c) => `${cd}${colName(c)}${rd}${r + 1}`;
      const fullCols = this._r0 === 0 && this._r1 === MAX_ROWS - 1;
      const fullRows = this._c0 === 0 && this._c1 === MAX_COLS - 1;
      if (fullCols && !fullRows) return `${cd}${colName(this._c0)}:${cd}${colName(this._c1)}`;
      if (fullRows && !fullCols) return `${rd}${this._r0 + 1}:${rd}${this._r1 + 1}`;
      if (this._r0 === this._r1 && this._c0 === this._c1) return one(this._r0, this._c0);
      return `${one(this._r0, this._c0)}:${one(this._r1, this._c1)}`;
    }
    GetWorksheet() { return newSheet(this._h); }
    GetRowsCount() { return this._r1 - this._r0 + 1; }
    GetColumnsCount() { return this._c1 - this._c0 + 1; }
    GetCellsCount() { return this._cells(); }
    /// Rows for a `GetRows()` range, columns for `GetCols()`, cells otherwise.
    GetCount() {
      if (this._kind === "rows") return this.GetRowsCount();
      if (this._kind === "cols") return this.GetColumnsCount();
      return this._cells();
    }
    get Count() { return this.GetCount(); }

    GetRows(nRow) {
      if (nRow === undefined) return this._sub(this._r0, this._c0, this._r1, this._c1, "rows");
      const n = intArg(nRow, "ApiRange.GetRows(nRow)");
      if (n < 1) throw new MacroError("RangeError", "ApiRange.GetRows counts from 1");
      const r = this._r0 + n - 1;
      if (r >= MAX_ROWS) return null;
      return this._sub(r, this._c0, r, this._c1);
    }
    GetCols(nCol) {
      if (nCol === undefined) return this._sub(this._r0, this._c0, this._r1, this._c1, "cols");
      const n = intArg(nCol, "ApiRange.GetCols(nCol)");
      if (n < 1) throw new MacroError("RangeError", "ApiRange.GetCols counts from 1");
      const c = this._c0 + n - 1;
      if (c >= MAX_COLS) return null;
      return this._sub(this._r0, c, this._r1, c);
    }
    GetEntireRow() { return this._sub(this._r0, 0, this._r1, MAX_COLS - 1); }
    GetEntireColumn() { return this._sub(0, this._c0, MAX_ROWS - 1, this._c1); }
    Offset(rowOffset, columnOffset) {
      const dr = intArg(rowOffset ?? 0, "rowOffset"), dc = intArg(columnOffset ?? 0, "columnOffset");
      const r0 = this._r0 + dr, c0 = this._c0 + dc, r1 = this._r1 + dr, c1 = this._c1 + dc;
      if (r0 < 0 || c0 < 0 || r1 >= MAX_ROWS || c1 >= MAX_COLS) return null;
      return this._sub(r0, c0, r1, c1);
    }
    Resize(rowSize, columnSize) {
      const nr = intArg(rowSize, "rowSize"), nc = intArg(columnSize, "columnSize");
      if (nr < 1 || nc < 1) return null;
      const r1 = this._r0 + nr - 1, c1 = this._c0 + nc - 1;
      if (r1 >= MAX_ROWS || c1 >= MAX_COLS) return null;
      return this._sub(this._r0, this._c0, r1, c1);
    }

    /// Calls `fCallback` with a one-cell range for each cell, row by row.
    /// A whole row or column stops at the used bounds — the cells past them
    /// are empty, and visiting a million of them is a hang, not a loop.
    ForEach(fCallback) {
      if (typeof fCallback !== "function") throw new MacroError("TypeError", "ApiRange.ForEach expects a function");
      const fullAxis = this._r1 === MAX_ROWS - 1 || this._c1 === MAX_COLS - 1;
      const u = fullAxis ? this._used() : { r0: this._r0, c0: this._c0, r1: this._r1, c1: this._c1, empty: false };
      if (u.empty) return true;
      for (let r = u.r0; r <= u.r1; r += 1) {
        for (let c = u.c0; c <= u.c1; c += 1) fCallback(this._sub(r, c, r, c));
      }
      return true;
    }

    // -- values

    _grid(read) {
      if (this._r0 === this._r1 && this._c0 === this._c1) return read(this._r0, this._c0);
      // The grid a whole row or column reads as is cut at the used bounds, as
      // ONLYOFFICE's is; a 1,048,576-row array of empty strings helps no one.
      const fullAxis = this._r1 === MAX_ROWS - 1 || this._c1 === MAX_COLS - 1;
      const u = fullAxis ? this._used() : { r0: this._r0, c0: this._c0, r1: this._r1, c1: this._c1, empty: false };
      if (u.empty) return [];
      this._bounded("GetValue");
      const out = [];
      for (let r = u.r0; r <= u.r1; r += 1) {
        const row = [];
        for (let c = u.c0; c <= u.c1; c += 1) row.push(read(r, c));
        out.push(row);
      }
      return out;
    }
    /// The cell's value without its number format, as a string — a formula's
    /// computed result, a date's serial number, `""` for an empty cell. This
    /// is ONLYOFFICE's behaviour (`getValueWithoutFormat`).
    _valueAt(r, c) {
      const sheet = this._sheet;
      if (E.session_cell_input(sheet, r, c) === "") return "";
      const shown = E.session_evaluate(sheet, `${colName(c)}${r + 1}`);
      // `session_evaluate` writes text the way Excel's dialogs show it, in
      // quotes; a value is wanted, not its display.
      if (shown.length >= 2 && shown.startsWith('"') && shown.endsWith('"')) {
        return shown.slice(1, -1).replace(/""/g, '"');
      }
      return shown;
    }
    GetValue() { return this._grid((r, c) => this._valueAt(r, c)); }
    GetValue2() { return this.GetValue(); }
    get Value() { return this.GetValue(); }
    GetText() {
      return this._grid((r, c) => {
        const items = JSON.parse(E.session_cells(this._sheet, r, c, r, c));
        const it = items.find((x) => x.r === r && x.c === c);
        return it && it.t != null ? String(it.t) : "";
      });
    }
    get Text() { return this.GetText(); }
    GetFormula() { return this._grid((r, c) => E.session_cell_input(this._sheet, r, c)); }

    /// A scalar fills every cell; a 2-D array fills cell by cell from the
    /// top-left (a 1-D array is one row), stopping at the range's edge.
    SetValue(data) {
      const sheet = this._sheet;
      const write = (r, c, v) => mutate("session_set_cell", sheet, r, c, entryText(v));
      if (Array.isArray(data)) {
        const rows = Array.isArray(data[0]) ? data : [data];
        for (let i = 0; i < rows.length && this._r0 + i <= this._r1; i += 1) {
          const row = Array.isArray(rows[i]) ? rows[i] : [rows[i]];
          for (let j = 0; j < row.length && this._c0 + j <= this._c1; j += 1) {
            write(this._r0 + i, this._c0 + j, row[j]);
          }
        }
        return true;
      }
      this._bounded("SetValue");
      const text = entryText(data);
      for (let r = this._r0; r <= this._r1; r += 1) {
        for (let c = this._c0; c <= this._c1; c += 1) write(r, c, text);
      }
      return true;
    }
    set Value(v) { this.SetValue(v); }
    /// Formula entry and typed entry are one path in the engine, as in the
    /// formula bar: `=` makes a formula.
    SetFormula(data) { return this.SetValue(data); }

    Clear() {
      const u = this._used();
      if (!u.empty) mutate("session_clear_range", this._sheet, u.r0, u.c0, u.r1, u.c1);
      return true;
    }
    ClearContents() {
      const u = this._used();
      if (!u.empty) mutate("session_clear_contents", this._sheet, u.r0, u.c0, u.r1, u.c1);
    }
    ClearFormats() {
      const u = this._used();
      if (!u.empty) mutate("session_clear_formats", this._sheet, u.r0, u.c0, u.r1, u.c1);
    }

    // -- formatting

    _format(r = this._r0, c = this._c0) {
      return JSON.parse(E.session_cell_format(this._sheet, r, c));
    }
    /// Whether every cell carries `flag` in `session_cell_format`. Cells past
    /// the used bounds carry no style, so a range reaching past them is not
    /// uniformly anything.
    _all(flag) {
      const b = usedBounds(this._sheet);
      if (this._r1 >= b.rows || this._c1 >= b.cols) return false;
      for (let r = this._r0; r <= this._r1; r += 1) {
        for (let c = this._c0; c <= this._c1; c += 1) if (!this._format(r, c)[flag]) return false;
      }
      return true;
    }
    /// Set a font flag through the toolbar's toggle, which sets the whole
    /// range one way and keeps every other property of each cell. A toggle
    /// sets "on" unless the range is already all on, so "off" over a mixed
    /// range is on-then-off — both halves inside the run's one undo step.
    _flag(fn, flag, on) {
      this._bounded(fn);
      const all = flag === "b"
        ? E.session_range_bold(this._sheet, this._r0, this._c0, this._r1, this._c1)
        : this._all(flag);
      if (on && all) return true;
      if (!on && !all) this._style(fn);
      if (!on || !all) this._style(fn);
      return true;
    }
    SetBold(isBold) { return this._flag("session_toggle_bold", "b", !!isBold); }
    SetItalic(isItalic) { return this._flag("session_toggle_italic", "i", !!isItalic); }
    SetStrikeout(isStrikeout) { return this._flag("session_toggle_strike", "st", !!isStrikeout); }
    SetUnderline(type) {
      const t = type === true ? "single" : type === false ? "none" : String(type);
      if (t === "none") return this._flag("session_toggle_underline", "u", false);
      if (t === "single") return this._flag("session_toggle_underline", "u", true);
      throw new NotSupported(`ApiRange.SetUnderline("${t}")`);
    }
    SetFontName(sName) { return this._style("session_set_font_name", String(sName)); }
    SetFontSize(nSize) {
      const n = Number(nSize);
      if (!(n > 0 && n <= 409)) throw new MacroError("RangeError", `font size ${nSize} is outside 1–409`);
      return this._style("session_set_font_size", n);
    }
    SetFontColor(oColor) { return this._style("session_set_font_color", hexOf(oColor, "SetFontColor"), -1, 0); }
    SetFillColor(oColor) { return this._style("session_set_fill", hexOf(oColor, "SetFillColor"), -1, 0); }
    /// The top-left cell's fill, or the string `'No Fill'`.
    GetFillColor() {
      const bg = this._format().bg;
      return (bg && colorFromHex(bg)) || "No Fill";
    }
    SetNumberFormat(sFormat) { return this._style("session_set_number_format", String(sFormat ?? "")); }
    GetNumberFormat() { return this._format().nf ?? "General"; }
    SetAlignHorizontal(s) {
      const v = String(s);
      if (!["left", "right", "center", "justify"].includes(v)) throw new MacroError("RangeError", `unknown horizontal alignment "${v}"`);
      return this._style("session_set_align", v);
    }
    SetAlignVertical(s) {
      const v = String(s);
      if (!["center", "bottom", "top", "distributed", "justify"].includes(v)) throw new MacroError("RangeError", `unknown vertical alignment "${v}"`);
      return this._style("session_set_valign", v);
    }
    SetWrap(isWrap) { return this._style("session_set_text_overflow", isWrap ? "wrap" : "overflow"); }
    GetWrapText() { return !!this._format().w; }
    /// ONLYOFFICE's `BordersIndex` and `LineStyle`, mapped to the line styles
    /// the editor's own border menu applies. A style the model has no exact
    /// counterpart for is refused rather than approximated.
    SetBorders(bordersIndex, lineStyle, oColor) {
      const kind = {
        Top: "top", Bottom: "bottom", Left: "left", Right: "right",
        InsideHorizontal: "horizontal", InsideVertical: "vertical",
        DiagonalDown: "diagdown", DiagonalUp: "diagup",
      }[bordersIndex];
      const style = {
        Thin: "thin", Medium: "medium", Thick: "thick", Dashed: "dashed", Dotted: "dotted", Double: "double",
      }[lineStyle];
      if (!kind || !style) throw new NotSupported(`ApiRange.SetBorders("${bordersIndex}", "${lineStyle}")`);
      return this._style("session_set_border", kind, style, hexOf(oColor, "SetBorders"));
    }

    // -- structure

    Merge(isAcross) {
      if (isAcross) {
        for (let r = this._r0; r <= this._r1; r += 1) mutate("session_merge_cells", this._sheet, r, this._c0, r, this._c1);
      } else {
        mutate("session_merge_cells", this._sheet, this._r0, this._c0, this._r1, this._c1);
      }
      return true;
    }
    UnMerge() {
      mutate("session_unmerge_cells", this._sheet, this._r0, this._c0, this._r1, this._c1);
      return true;
    }
    /// Width in ONLYOFFICE's unit: characters of the default font, one being
    /// 7 px, plus the 5 px cell padding Excel adds.
    SetColumnWidth(nWidth) {
      const px = Math.max(0, Math.round(Number(nWidth) * 7) + 5);
      for (let c = this._c0; c <= this._c1; c += 1) mutate("session_set_col_width", this._sheet, c, px);
    }
    GetColumnWidth() { return Math.max(0, (E.session_col_width(this._sheet, this._c0) - 5) / 7); }
    /// Points, as ONLYOFFICE's reference says; the engine sizes in px at 96 dpi.
    SetRowHeight(nHeight) {
      const px = Math.max(0, Math.round((Number(nHeight) * 96) / 72));
      for (let r = this._r0; r <= this._r1; r += 1) mutate("session_set_row_height", this._sheet, r, px);
      return true;
    }
    GetRowHeight() { return (E.session_row_height(this._sheet, this._r0) * 72) / 96; }
    /// "The specified range must span an entire column or row."
    SetHidden(isHidden) {
      const sheet = this._sheet;
      if (this._c0 === 0 && this._c1 === MAX_COLS - 1) {
        mutate(isHidden ? "session_hide_rows" : "session_unhide_rows", sheet, this._r0, this._r1);
      } else if (this._r0 === 0 && this._r1 === MAX_ROWS - 1) {
        mutate(isHidden ? "session_hide_cols" : "session_unhide_cols", sheet, this._c0, this._c1);
      } else {
        throw new MacroError("RangeError", "ApiRange.SetHidden needs whole rows or whole columns");
      }
      return true;
    }
    GetHidden() {
      const sheet = this._sheet;
      if (this._r0 === 0 && this._r1 === MAX_ROWS - 1 && !(this._c0 === 0 && this._c1 === MAX_COLS - 1)) {
        return JSON.parse(E.session_col_px(sheet, this._c0, 1))[0] === 0;
      }
      return JSON.parse(E.session_row_px(sheet, this._r0, 1))[0] === 0;
    }

    /// The selection is the editor's, not the workbook's: recorded as a UI
    /// step applied after the batch, never as an edit.
    Select() {
      ctx.ui({ op: "select", sheet: this._sheet, r0: this._r0, c0: this._c0, r1: this._r1, c1: this._c1 });
      ctx.selection = { sheet: this._sheet, r0: this._r0, c0: this._c0, r1: this._r1, c1: this._c1, row: this._r0, col: this._c0 };
      return true;
    }
  }
  const newRange = (h, r0, c0, r1, c1, kind) => guarded(new ApiRange(h, r0, c0, r1, c1, kind), "ApiRange");

  // ---- ApiFreezePanes ------------------------------------------------------

  class ApiFreezePanes {
    constructor(h) { this._h = h; }
    GetClassType() { return "freezePanes"; }
    /// "The range provided corresponds to the cells that will be frozen in the
    /// top- and left-most pane" — so its far corner is the last frozen cell.
    FreezeAt(frozenRange) {
      const rg = typeof frozenRange === "string" ? newSheet(this._h).GetRange(frozenRange) : frozenRange;
      if (!rg || rg._r1 === undefined) throw new MacroError("TypeError", "FreezeAt expects a range");
      mutate("session_set_freeze", this._h.index, rg._r1 + 1, rg._c1 + 1);
    }
    FreezeColumns(count) {
      mutate("session_set_freeze", this._h.index, 0, intArg(count ?? 1, "count"));
    }
    FreezeRows(count) {
      mutate("session_set_freeze", this._h.index, intArg(count ?? 1, "count"), 0);
    }
    Unfreeze() { mutate("session_set_freeze", this._h.index, 0, 0); }
    GetLocation() {
      const f = JSON.parse(E.session_frozen(this._h.index));
      if (!f.rows && !f.cols) return null;
      return newRange(this._h, 0, 0, Math.max(0, f.rows - 1), Math.max(0, f.cols - 1));
    }
  }

  // ---- ApiWorksheet --------------------------------------------------------

  class ApiWorksheet {
    constructor(h) { this._h = h; }
    get _index() {
      if (this._h.deleted) throw new MacroError("InvalidReference", "the worksheet was deleted");
      return this._h.index;
    }
    GetClassType() { return "worksheet"; }
    GetName() { return sheetNames()[this._index]; }
    get Name() { return this.GetName(); }
    SetName(sName) { mutate("session_rename_sheet", this._index, String(sName)); }
    set Name(v) { this.SetName(v); }
    GetIndex() { return this._index; }
    get Index() { return this._index; }
    GetVisible() { return JSON.parse(E.session_sheet_visibility())[this._index] === "visible"; }
    SetVisible(isVisible) {
      mutate("session_set_sheet_visibility", this._index, isVisible ? "visible" : "hidden");
      return true;
    }
    SetActive() {
      ctx.ui({ op: "activate", sheet: this._index });
      ctx.selection = { sheet: this._index, r0: 0, c0: 0, r1: 0, c1: 0, row: 0, col: 0 };
      return true;
    }
    Delete() {
      const i = this._index;
      mutate("session_delete_sheet", i);
      this._h.deleted = true;
      for (const h of handles) if (!h.deleted && h.index > i) h.index -= 1;
      return true;
    }

    /// `GetRange("A1:B2")`, `GetRange("A1", "C3")`, or ranges in place of
    /// either string.
    GetRange(range1, range2) {
      const one = (x) => {
        if (x && typeof x === "object" && x._r0 !== undefined) return x;
        const p = parseRef(x);
        if (!p) return null;
        if (p.sheetName !== null) {
          const i = sheetNames().indexOf(p.sheetName);
          if (i < 0) return null;
          return newRange(sheetHandle(i), p.r0, p.c0, p.r1, p.c1);
        }
        return newRange(this._h, p.r0, p.c0, p.r1, p.c1);
      };
      const a = one(range1);
      if (!a) return null;
      if (range2 === undefined) return a;
      const b = one(range2);
      if (!b) return null;
      return newRange(a._h, Math.min(a._r0, b._r0), Math.min(a._c0, b._c0), Math.max(a._r1, b._r1), Math.max(a._c1, b._c1));
    }
    /// 0-based, as ONLYOFFICE's samples use it (column A is 0).
    GetRangeByNumber(nRow, nCol) {
      const r = intArg(nRow, "nRow"), c = intArg(nCol, "nCol");
      if (r < 0 || c < 0 || r >= MAX_ROWS || c >= MAX_COLS) return null;
      return newRange(this._h, r, c, r, c);
    }
    /// A number is a 1-based row; a string is a row range like `"1:4"`.
    GetRows(value) {
      if (typeof value === "number") {
        const n = intArg(value, "GetRows(value)");
        if (n < 1 || n > MAX_ROWS) return null;
        return newRange(this._h, n - 1, 0, n - 1, MAX_COLS - 1, "rows");
      }
      if (value === undefined) return newRange(this._h, 0, 0, MAX_ROWS - 1, MAX_COLS - 1, "rows");
      const p = parseRef(value);
      if (!p) return null;
      return newRange(this._h, p.r0, 0, p.r1, MAX_COLS - 1, "rows");
    }
    /// Whole columns spanned by a reference such as `"A1:C1"` or `"A:C"`.
    GetCols(sRange) {
      if (sRange === undefined) return newRange(this._h, 0, 0, MAX_ROWS - 1, MAX_COLS - 1, "cols");
      const p = parseRef(sRange);
      if (!p) return null;
      return newRange(this._h, 0, p.c0, MAX_ROWS - 1, p.c1, "cols");
    }
    /// From A1 to the last used row and column — the shape ONLYOFFICE's own
    /// samples index into with `GetRangeByNumber(row, col)` from 0.
    GetUsedRange() {
      const b = usedBounds(this._index);
      return newRange(this._h, 0, 0, Math.max(0, b.rows - 1), Math.max(0, b.cols - 1));
    }
    GetSelection() {
      const s = ctx.selection;
      if (!s || s.sheet !== this._index) return newRange(this._h, 0, 0, 0, 0);
      return newRange(this._h, s.r0, s.c0, s.r1, s.c1);
    }
    get Selection() { return this.GetSelection(); }
    GetActiveCell() {
      const s = ctx.selection;
      if (!s || s.sheet !== this._index) return newRange(this._h, 0, 0, 0, 0);
      return newRange(this._h, s.row, s.col, s.row, s.col);
    }
    get ActiveCell() { return this.GetActiveCell(); }
    /// 0-based column, width in characters (one = 7 px) plus the 5 px padding
    /// unless `bWithoutPaddings`.
    SetColumnWidth(nColumn, nWidth, bWithoutPaddings) {
      const c = intArg(nColumn, "nColumn");
      const px = Math.max(0, Math.round(Number(nWidth) * 7) + (bWithoutPaddings ? 0 : 5));
      mutate("session_set_col_width", this._index, c, px);
      return true;
    }
    /// 0-based row, height in points.
    SetRowHeight(nRow, nHeight) {
      const r = intArg(nRow, "nRow");
      mutate("session_set_row_height", this._index, r, Math.max(0, Math.round((Number(nHeight) * 96) / 72)));
      return true;
    }
    SetDisplayGridlines(isDisplayed) {
      mutate("session_set_gridlines_hidden", this._index, !isDisplayed);
      return true;
    }
    GetFreezePanes() { return guarded(new ApiFreezePanes(this._h), "ApiFreezePanes"); }
  }
  const newSheet = (h) => guarded(new ApiWorksheet(h), "ApiWorksheet");

  // ---- Api -----------------------------------------------------------------

  const activeIndex = () => ctx.selection?.sheet ?? 0;

  class ApiRoot {
    GetActiveSheet() { return newSheet(sheetHandle(activeIndex())); }
    get ActiveSheet() { return this.GetActiveSheet(); }
    GetSheets() { return sheetNames().map((_, i) => newSheet(sheetHandle(i))); }
    get Sheets() { return this.GetSheets(); }
    /// A name or a 0-based index; `null` when there is no such sheet.
    GetSheet(nameOrIndex) {
      const names = sheetNames();
      const i = typeof nameOrIndex === "number" ? nameOrIndex : names.indexOf(String(nameOrIndex));
      if (!Number.isInteger(i) || i < 0 || i >= names.length) return null;
      return newSheet(sheetHandle(i));
    }
    /// "The new worksheet becomes the active sheet."
    AddSheet(sName) {
      const i = mutate("session_add_sheet");
      if (sName !== undefined && sName !== null && String(sName) !== sheetNames()[i]) {
        mutate("session_rename_sheet", i, String(sName));
      }
      ctx.ui({ op: "activate", sheet: i });
      ctx.selection = { sheet: i, r0: 0, c0: 0, r1: 0, c1: 0, row: 0, col: 0 };
      return newSheet(sheetHandle(i));
    }
    GetRange(sRange) { return this.GetActiveSheet().GetRange(sRange); }
    GetSelection() { return this.GetActiveSheet().GetSelection(); }
    get Selection() { return this.GetSelection(); }
    CreateColorFromRGB(r, g, b) { return newColor(Number(r), Number(g), Number(b)); }
    RGB(r, g, b) { return newColor(Number(r), Number(g), Number(b)); }
    HexColor(hexString) {
      const c = colorFromHex(hexString);
      if (!c) throw new MacroError("TypeError", `"${hexString}" is not a hex colour`);
      return c;
    }
    RecalculateAllFormulas() {
      mutate("session_recalculate");
      return true;
    }
  }

  return guarded(new ApiRoot(), "Api");
}

/// The engine calls a recorded operation may name. The main thread replays
/// only these: an operation list is data from a sandbox, and a name outside
/// this set is refused rather than looked up on the engine.
export const REPLAYABLE = new Set([
  "session_set_cell",
  "session_clear_range",
  "session_clear_contents",
  "session_clear_formats",
  "session_toggle_bold",
  "session_toggle_italic",
  "session_toggle_strike",
  "session_toggle_underline",
  "session_set_font_name",
  "session_set_font_size",
  "session_set_font_color",
  "session_set_fill",
  "session_set_number_format",
  "session_set_align",
  "session_set_valign",
  "session_set_text_overflow",
  "session_set_border",
  "session_merge_cells",
  "session_unmerge_cells",
  "session_set_col_width",
  "session_set_row_height",
  "session_hide_rows",
  "session_unhide_rows",
  "session_hide_cols",
  "session_unhide_cols",
  "session_set_freeze",
  "session_rename_sheet",
  "session_set_sheet_visibility",
  "session_delete_sheet",
  "session_add_sheet",
  "session_set_gridlines_hidden",
  "session_recalculate",
]);
