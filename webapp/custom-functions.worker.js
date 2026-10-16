// The custom-function sandbox (`MACRO-04`, docs/99 §5).
//
// One worker per workbook's set of sources, long-lived, because a function is
// called once per cell and a worker per call would pay a start-up per cell.
// It never sees the workbook: a custom function is a function of its
// arguments (and of `this.address`), so it gets those and nothing else — no
// engine, no snapshot, and of the Office API only `Api.AddCustomFunction`.
//
// The protocol, page → worker:
//
// * `{type:"load", sources:[{guid, name, source}]}` — once, first. Every
//   source is compiled, the sandbox is closed (the same lockdown a macro
//   runs under: no network, storage, DOM, `eval` or `import`), then each
//   source runs and its `Api.AddCustomFunction` calls register functions.
//   Answered with `{type:"loaded", functions, errors}`.
// * `{type:"call", ticket, name, args, address, argAddresses}` — answered
//   with `{type:"started", ticket}` as the function is entered and
//   `{type:"result", ticket, value}` when it returns or its promise settles.
//
// A function that never returns is not this worker's problem to solve and it
// cannot: the page times each call from `started` and terminates the worker,
// which nothing inside it can prevent.

import { MacroError } from "./macro.api.js";
import { lockdown } from "./macro.sandbox.js";
import { CellError, convertAnswer, convertArgument, describe, parseJsDoc, versionOf } from "./custom-functions.js";

/// Taken before user code exists; the globals are closed by `lockdown`.
const post = self.postMessage.bind(self);
const listen = self.addEventListener.bind(self);

/// NAME → { fn, meta }
const registered = new Map();
let loaded = false;

function compile(source) {
  if (/\bimport\b/.test(source)) {
    throw new MacroError("SandboxViolation", "import is not available to macros (remove the word `import`)");
  }
  // eslint-disable-next-line no-new-func
  return new Function("Api", source);
}

function lineOf(err) {
  const m = /<anonymous>:(\d+):(\d+)/.exec(String(err && err.stack));
  return m ? Math.max(1, Number(m[1]) - 2) : null;
}

/// What a source sees as `Api`: `AddCustomFunction`, and a named refusal for
/// everything else — a custom function computes a value, it does not edit.
function apiFor(entry, docs, out, errors) {
  const version = versionOf(entry.source);
  const add = function AddCustomFunction(fn) {
    const doc = docs.shift();
    const meta = describe(fn, doc, version);
    if (meta.error) {
      // ONLYOFFICE throws here, which stops the rest of the source.
      throw new MacroError("CustomFunctionError", meta.error);
    }
    meta.guid = entry.guid;
    registered.set(meta.name, { fn, meta });
    out.push(meta);
  };
  return new Proxy(
    {},
    {
      get(_, prop) {
        if (prop === "AddCustomFunction") return add;
        if (typeof prop !== "string" || prop === "then") return undefined;
        return () => {
          throw new MacroError("NotSupported", `Api.${prop} is not available while custom functions are registered`);
        };
      },
    },
  );
}

function load(sources) {
  const functions = [];
  const errors = [];
  const compiled = [];
  for (const entry of sources) {
    try {
      compiled.push({ entry, run: compile(String(entry.source ?? "")) });
    } catch (err) {
      errors.push({ guid: entry.guid, name: entry.name, message: `${err.name}: ${err.message}`, line: lineOf(err) });
    }
  }
  lockdown();
  for (const { entry, run } of compiled) {
    const source = String(entry.source ?? "");
    // ONLYOFFICE parses the JSDoc only when the source carries the tag, and
    // keeps no more blocks than there are calls.
    const docs = source.includes("@customfunction") ? parseJsDoc(source) : [];
    const calls = (source.match(/\.AddCustomFunction\(/g) || []).length;
    docs.length = Math.min(docs.length, calls);
    try {
      run.call(undefined, apiFor(entry, docs, functions, errors));
    } catch (err) {
      const message = err && err.message ? String(err.message) : String(err);
      errors.push({ guid: entry.guid, name: entry.name, message, line: lineOf(err) });
    }
  }
  return { functions, errors };
}

async function call({ ticket, name, args, address, argAddresses }) {
  const entry = registered.get(name);
  if (!entry) return { error: "#NAME?" };
  const { fn, meta } = entry;
  let converted;
  try {
    converted = meta.params.map((p, i) => convertArgument(i < args.length ? args[i] : undefined, p));
  } catch (err) {
    if (err instanceof CellError) return { error: err.token };
    return { error: "#VALUE!" };
  }
  const context = {
    address,
    args: (argAddresses || []).map((a) => (a ? { address: a } : undefined)),
  };
  post({ type: "started", ticket });
  try {
    let out = fn.apply(context, converted);
    if (out && typeof out.then === "function") out = await out;
    return convertAnswer(out, meta.returns);
  } catch (err) {
    // An unhandled exception is #VALUE!, as in Excel and ONLYOFFICE.
    return { error: "#VALUE!", message: err && err.message ? String(err.message) : String(err) };
  }
}

listen("message", async (ev) => {
  const m = ev.data || {};
  if (m.type === "load") {
    if (loaded) return;
    loaded = true;
    let result;
    try {
      result = load(Array.isArray(m.sources) ? m.sources : []);
    } catch (err) {
      result = { functions: [], errors: [{ message: `${err.name}: ${err.message}` }] };
    }
    post({
      type: "loaded",
      functions: result.functions.map(({ guid, ...meta }) => ({ ...meta, guid })),
      errors: result.errors,
    });
  } else if (m.type === "call" && loaded) {
    const value = await call(m);
    const message = value && typeof value === "object" && !Array.isArray(value) ? value.message : undefined;
    if (message !== undefined) delete value.message;
    post({ type: "result", ticket: m.ticket, value, message });
  }
});
