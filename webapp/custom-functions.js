// Custom worksheet functions (`MACRO-04`): the rules, as plain functions.
//
// No DOM and no engine here, so both the sandboxed worker and the editor can
// import it, and so the rules can be read in one place. Each is ONLYOFFICE's,
// taken from its source rather than guessed (sdkjs, 2026-10):
//
// * **JSDoc** — `common/apiCommon.js` `parseJSDoc`: every `/** … */` block in
//   the source, in order; `@param {type} name description`, `[name]` or
//   `{?type}` for optional, `[name=default]` for a default; `@returns {type}`;
//   any other `@tag` recorded. The *i*-th `Api.AddCustomFunction` call takes
//   the *i*-th block (`common/macros.js`), and a call with no valid block is
//   refused: `Invalid parameters type in JSDOC or options.`
//   (`cell/apiBuilder.js` `Api.AddCustomFunction`).
// * **Arguments** — `cell/model/WorkbookElems.js` `CCustomFunctionEngine`:
//   names are upper-cased; the parameter list is read from the function's own
//   text; a parameter's type comes from the JSDoc entry of the same name, else
//   `any`; trailing optional parameters lower the minimum; `prepareInputArg`
//   converts each argument to its declared type, and a range given where a
//   scalar is declared is `#VALUE!`.
// * **Context** — `this.address` is the calling cell, `this.args[i].address`
//   each argument's, both sheet-qualified (`Sheet1!A1`); a non-reference
//   argument has no entry.
// * **Answers** — `prepareResult`: a Promise is awaited; an object where a
//   scalar is declared is `#VALUE!`; a `T[][]` answer must be an array (a flat
//   one is one row), and an element spelled like an error is that error.
// * **Volatility** — `@calculateCell` (ONLYOFFICE's tag) or `@volatile`
//   (Excel's) make a function recalculate every time; otherwise an answer is
//   kept for as long as its arguments are unchanged.
//
// Where this differs from ONLYOFFICE, deliberately:
//
// * A left-out optional argument with no default is `null` (Excel's
//   documented default, and what ONLYOFFICE's own sample tests for), not
//   skipped — ONLYOFFICE drops it from the argument list, which shifts every
//   later argument into the wrong parameter.
// * A missing `@returns` reads as `any` rather than turning every answer into
//   `#VALUE!`.
// * `undefined` and `null` answers are an empty cell rather than `#VALUE!`.

export const TYPES = new Set([
  "number", "string", "boolean", "any",
  "number[]", "string[]", "boolean[]", "any[]",
  "number[][]", "string[][]", "boolean[][]", "any[][]",
]);

const ERROR_TOKENS = new Set([
  "#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#SPILL!", "#CALC!", "#BUSY!",
]);

/// Every JSDoc block in `source`, in order — ONLYOFFICE's parser, kept to the
/// fields custom functions use.
export function parseJsDoc(source) {
  const out = [];
  const blockRe = /\/\*\*([\s\S]*?)\*\//g;
  let m;
  while ((m = blockRe.exec(source)) !== null) {
    const body = m[1].trim();
    const params = [];
    const paramRe = /@param\s+{(\??)(.+?)}\s+(?:(\[)?([\w.]+|\{[\w.]+\}))?(?:\s*=\s*([^@]+?)(?=\s|\)|$))?(?:\s+(.+))?/g;
    let p;
    while ((p = paramRe.exec(body)) !== null) {
      const optional = p[3] !== undefined || p[1] === "?";
      params.push({
        type: p[2].trim(),
        name: p[4] || "",
        optional,
        defaultValue: optional && p[5] !== undefined ? p[5].trim().replace(/\]$/, "") : undefined,
        description: (p[6] || "").replace(/^-\s*/, "").trim(),
      });
    }
    const r = /@returns?\s+{(.+?)}(?:\s+(.+))?/.exec(body);
    const tags = {};
    const tagRe = /@(\w+)/g;
    let t;
    while ((t = tagRe.exec(body)) !== null) {
      if (!["param", "property", "returns", "return"].includes(t[1])) tags[t[1]] = true;
    }
    const d = /\*\s*(.*)/.exec(body);
    let description = d ? d[1].trim() : "";
    if (description.startsWith("@")) description = "";
    out.push({ params, returns: r ? { type: r[1].trim(), description: (r[2] || "").trim() } : null, tags, description });
  }
  return out;
}

/// `apiBuilder.js` `private_ValidateParamsForCustomFunction`.
export function validJsDoc(doc) {
  if (!doc) return false;
  if (doc.returns && !TYPES.has(doc.returns.type)) return false;
  return doc.params.every((p) => TYPES.has(p.type));
}

/// A function's parameter names, from its own text (ONLYOFFICE
/// `_getArgsByFunc`).
export function parameterNames(fn) {
  const text = Function.prototype.toString.call(fn).replace(/((\/\/.*$)|(\/\*[\s\S]*?\*\/))/gm, "");
  const open = text.indexOf("(");
  const close = text.indexOf(")");
  if (open < 0 || close < open) return [];
  return (text.slice(open + 1, close).match(/([^\s,]+)/g) || []).map((n) => n.replace(/=.*$/, ""));
}

/// FNV-1a over the text, as hex: a version that changes whenever the source
/// does, so every answer computed by the old source is asked for again.
export function versionOf(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0") + ":" + text.length;
}

/// The registration record for `fn`, as the engine wants it, or an error
/// message. `doc` is the JSDoc block this call took.
export function describe(fn, doc, version) {
  if (typeof fn !== "function") return { error: "Api.AddCustomFunction needs a function" };
  if (!validJsDoc(doc)) return { error: "Invalid parameters type in JSDOC or options." };
  const name = String(fn.name || "").toUpperCase();
  if (!name) return { error: "a custom function needs a name: write `function NAME(…)`" };
  if (!/^[A-Z_][A-Z0-9_.]*$/.test(name) || name.length > 255) return { error: `${name} is not a function name` };
  const names = parameterNames(fn);
  const byName = new Map(doc.params.map((p) => [p.name, p]));
  const params = names.map((n) => {
    const p = byName.get(n);
    return {
      name: n,
      type: p?.type ?? "any",
      optional: !!p?.optional,
      defaultValue: p?.defaultValue,
      description: p?.description ?? "",
    };
  });
  let trailing = 0;
  for (const p of params) trailing = p.optional ? trailing + 1 : 0;
  return {
    name,
    version,
    minArgs: params.length - trailing,
    maxArgs: params.length,
    volatile: !!(doc.tags.volatile || doc.tags.calculateCell),
    async: Object.prototype.toString.call(fn) === "[object AsyncFunction]",
    description: doc.description,
    params,
    returns: doc.returns?.type ?? "any",
  };
}

// ---- Arguments in, answers out ---------------------------------------------

export class CellError extends Error {
  constructor(token) {
    super(token);
    this.token = token;
  }
}

const isError = (v) => v !== null && typeof v === "object" && !Array.isArray(v) && typeof v.error === "string";

function toNumber(v) {
  if (isError(v)) throw new CellError(v.error);
  if (v === null || v === undefined) return 0;
  if (typeof v === "number") return v;
  if (typeof v === "boolean") return v ? 1 : 0;
  const s = String(v).trim();
  if (s !== "" && Number.isFinite(Number(s))) return Number(s);
  throw new CellError("#VALUE!");
}
function toText(v) {
  if (isError(v)) throw new CellError(v.error);
  if (v === null || v === undefined) return "";
  if (typeof v === "boolean") return v ? "TRUE" : "FALSE";
  return String(v);
}
function toBool(v) {
  if (isError(v)) throw new CellError(v.error);
  if (v === null || v === undefined) return false;
  if (typeof v === "boolean") return v;
  if (typeof v === "number") return v !== 0;
  const s = String(v).toLowerCase();
  if (s === "true" || s === "false") return s === "true";
  throw new CellError("#VALUE!");
}
function toAny(v) {
  return isError(v) ? v.error : v;
}
const SCALAR = { number: toNumber, string: toText, boolean: toBool, any: toAny };

/// One argument converted to its declared `type`. Throws `CellError` with the
/// cell's answer when it cannot be.
export function convertArgument(value, param) {
  const type = param?.type ?? "any";
  if (value === undefined) {
    // Left out of the call.
    if (param?.defaultValue === undefined) return null;
    value = param.defaultValue;
  }
  const matrix = type.endsWith("[]");
  const base = type.replace(/(\[\])+$/, "");
  const one = SCALAR[base] ?? toAny;
  if (!matrix) {
    if (Array.isArray(value)) throw new CellError("#VALUE!");
    return one(value);
  }
  const rows = Array.isArray(value) ? value : [[value]];
  // An error inside a block is the function's to see: as its token for `any`,
  // as the error for a typed block.
  const block = rows.map((row) => row.map((v) => (isError(v) && base !== "any" ? v.error : one(v))));
  return type.endsWith("[][]") ? block : block.flat();
}

/// An answer converted by the declared return `type` into what the engine
/// reads (`custom.rs` in the wasm crate).
export function convertAnswer(value, type) {
  const t = TYPES.has(type) ? type : "any";
  if (t.endsWith("[]")) {
    if (!Array.isArray(value) || value.length === 0) return { error: "#VALUE!" };
    const rows = value.every((r) => !Array.isArray(r)) ? [value] : value;
    const base = t.replace(/(\[\])+$/, "");
    const out = [];
    for (const row of rows) {
      if (!Array.isArray(row)) return { error: "#VALUE!" };
      const line = [];
      for (const v of row) {
        if (v !== null && typeof v === "object") return { error: "#VALUE!" };
        if (typeof v === "string" && ERROR_TOKENS.has(v)) return { error: v };
        line.push(scalarAnswer(v, base));
      }
      out.push(line);
    }
    return out;
  }
  if (value === undefined || value === null) return null;
  if (typeof value === "object") return { error: "#VALUE!" };
  return scalarAnswer(value, t);
}

function scalarAnswer(v, base) {
  if (v === undefined || v === null) return null;
  if (typeof v === "number") return Number.isFinite(v) ? v : { error: "#NUM!" };
  if (typeof v === "boolean") return base === "string" ? (v ? "TRUE" : "FALSE") : v;
  if (typeof v === "bigint") return Number(v);
  if (base === "boolean" || base === "any") {
    const s = String(v).toLowerCase();
    if (s === "true" || s === "false") return s === "true";
  }
  return String(v);
}
