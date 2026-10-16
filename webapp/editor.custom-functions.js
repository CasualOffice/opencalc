// Custom worksheet functions (`MACRO-04`, docs/99): the editor's half.
//
// The engine never runs JavaScript. A recalculation that meets a call it has
// no answer for evaluates the cell to `#BUSY!` and lists the call; this module
// runs the call in the custom-function sandbox (`custom-functions.worker.js`)
// and hands the answer back, and the engine recalculates the cell and what
// reads it. Answers are not document data: nothing here is an edit, nothing is
// sent to collaborators, and each client computes its own.
//
// ## When code runs
//
// docs/99 §3: scripts never run on open. A workbook that arrives carrying
// custom-function sources — opened, or joined over collaboration — keeps the
// values its file cached (the engine shows a FILE_CACHED value for a formula it
// cannot compute) until the person chooses **Enable** on the message bar. A
// workbook that arrives without any, or whose functions the person writes in
// the Macros window, is trusted for the rest of the tab: later source changes,
// a peer's included, are registered as they arrive — they run in the same
// sandbox, which can reach nothing but its arguments. Never where the mode
// withholds `canRunMacros` (embedded, WOPI, the viewer) and never read-only.
//
// ## Bounds
//
// One call runs at a time, each timed from the moment the function is entered;
// one that runs past `CUSTOM_FUNCTION_TIME_LIMIT_MS` is stopped by terminating
// the worker — the only stop nothing inside it can prevent — and answers
// `#VALUE!`, Excel's answer for a custom function that failed (an unhandled
// exception is `#VALUE!` in Excel and in ONLYOFFICE; neither documents a
// timeout of its own). The worker is then restarted for the next call. The
// engine bounds how many calls wait at once, and the answers it keeps.

import {
  BUILD,
  byId,
  draw,
  el,
  getCapabilities,
  status,
  statusError,
  wasm,
} from "./editor.core.js";

/// Per call. Generous for computation; a loop that never ends is cut off well
/// before anybody wonders whether the editor has hung (it has not: the call
/// runs off the main thread).
export const CUSTOM_FUNCTION_TIME_LIMIT_MS = 2_000;
/// For running the sources to register their functions.
const LOAD_LIMIT_MS = 10_000;

const cf = {
  limitMs: CUSTOM_FUNCTION_TIME_LIMIT_MS,
  /// What the last tick saw: `instance:generation`.
  seen: null,
  /// Runtimes (workbook instances) whose functions may run.
  trusted: new Set(),
  /// Runtimes the person said "Not now" to.
  declined: new Set(),
  worker: null,
  /// The sources the worker was loaded with, for a restart.
  sources: [],
  registering: false,
  pumping: false,
  /// The registration's problems, for the Macros window.
  errors: [],
  /// Bumped by every (re)registration; a pump from an older one stops.
  epoch: 0,
};

function readStatus() {
  try { return JSON.parse(wasm.session_custom_status()); } catch { return null; }
}

function librarySources() {
  try {
    const lib = JSON.parse(wasm.session_macros());
    return Array.isArray(lib.customFunctions) ? lib.customFunctions : [];
  } catch {
    return [];
  }
}

/// Whether this editor may run a workbook's code at all.
export function mayRunCustomFunctions() {
  const caps = getCapabilities();
  if (caps.readOnly || caps.canRunMacros === false) return false;
  let readOnly = false;
  try { readOnly = !!wasm.session_read_only(); } catch {}
  return !readOnly;
}

function stopWorker() {
  try { cf.worker?.terminate(); } catch {}
  cf.worker = null;
}

/// A fresh sandbox loaded with `sources`. Resolves `{functions, errors}`.
function startWorker(sources) {
  stopWorker();
  const worker = new Worker(new URL(`./custom-functions.worker.js?b=${BUILD}`, import.meta.url), { type: "module" });
  cf.worker = worker;
  cf.sources = sources;
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      if (cf.worker === worker) stopWorker();
      resolve({ functions: [], errors: [{ message: `TimeLimitExceeded: the custom functions took longer than ${LOAD_LIMIT_MS} ms to register and were stopped` }] });
    }, LOAD_LIMIT_MS);
    worker.onerror = (e) => {
      e.preventDefault?.();
      clearTimeout(timer);
      resolve({ functions: [], errors: [{ message: e.message || "the custom-function worker failed" }] });
    };
    worker.onmessage = (ev) => {
      if (ev.data?.type !== "loaded") return;
      clearTimeout(timer);
      resolve({ functions: ev.data.functions || [], errors: ev.data.errors || [] });
    };
    worker.postMessage({ type: "load", sources: sources.map(({ guid, name, source }) => ({ guid, name, source })) });
  });
}

/// Run the sources and register what they declare. A name a built-in already
/// has is refused here and reported: the built-in would win in every formula
/// anyway, and a function nobody can call should not look registered.
async function register() {
  cf.registering = true;
  cf.epoch++;
  try {
    const sources = librarySources();
    let functions = [];
    let errors = [];
    if (sources.length) {
      ({ functions, errors } = await startWorker(sources));
    } else {
      stopWorker();
      // Nothing to register and nothing registered: registering "nothing"
      // would recalculate the whole workbook for no change.
      if (!(readStatus()?.registered > 0)) { cf.errors = []; return; }
    }
    let builtins = new Set();
    try { builtins = new Set(JSON.parse(wasm.function_catalog()).map((f) => f.n)); } catch {}
    const seen = new Set();
    const accepted = [];
    for (const f of functions) {
      if (builtins.has(f.name)) {
        errors.push({ guid: f.guid, message: `${f.name} is a built-in function; choose another name` });
      } else if (seen.has(f.name)) {
        errors.push({ guid: f.guid, message: `${f.name} is registered twice; the first one is used` });
      } else {
        seen.add(f.name);
        accepted.push(f);
      }
    }
    cf.errors = errors;
    try {
      wasm.session_set_custom_functions(JSON.stringify(accepted));
    } catch (e) {
      cf.errors.push({ message: String(e?.message ?? e) });
    }
    if (errors.length) statusError(`custom functions: ${errors[0].message}${errors.length > 1 ? ` (and ${errors.length - 1} more)` : ""}`);
  } finally {
    cf.registering = false;
  }
  draw();
}

/// One call in the sandbox. Resolves the answer the engine reads.
function runCall(call) {
  return new Promise((resolve) => {
    const worker = cf.worker;
    if (!worker) { resolve({ error: "#BUSY!" }); return; }
    let timer = null;
    const done = (value) => {
      clearTimeout(timer);
      worker.removeEventListener("message", onMessage);
      resolve(value);
    };
    const onMessage = (ev) => {
      const m = ev.data || {};
      if (m.ticket !== call.ticket) return;
      if (m.type === "started") {
        clearTimeout(timer);
        timer = setTimeout(() => {
          // Stopped from outside; the next call gets a new sandbox.
          if (cf.worker === worker) stopWorker();
          status.textContent = `custom function ${call.name} ran longer than ${cf.limitMs} ms and was stopped`;
          done({ error: "#VALUE!" });
        }, cf.limitMs);
      } else if (m.type === "result") {
        if (m.message) status.textContent = `custom function ${call.name}: ${m.message}`;
        done(m.value);
      }
    };
    worker.addEventListener("message", onMessage);
    // Until the function is entered, the clock is the worker's start-up.
    timer = setTimeout(() => done({ error: "#VALUE!" }), LOAD_LIMIT_MS);
    worker.postMessage({ type: "call", ...call });
  });
}

/// Answer every call the engine is waiting on, one at a time, until it asks
/// for no more.
async function pump() {
  if (cf.pumping) return;
  cf.pumping = true;
  const epoch = cf.epoch;
  try {
    for (;;) {
      let calls = [];
      try { calls = JSON.parse(wasm.session_custom_calls()); } catch { calls = []; }
      if (!calls.length) break;
      for (const call of calls) {
        if (cf.epoch !== epoch) return;
        if (!cf.worker) await startWorker(cf.sources);
        const value = await runCall(call);
        if (cf.epoch !== epoch) return;
        try { wasm.session_set_custom_results(JSON.stringify([{ ticket: call.ticket, value }])); } catch {}
        scheduleDraw();
      }
    }
  } finally {
    cf.pumping = false;
  }
  // A registration during the pump leaves calls for its own pump.
  if (cf.epoch !== epoch) queueMicrotask(tick);
}

let drawQueued = false;
function scheduleDraw() {
  if (drawQueued) return;
  drawQueued = true;
  requestAnimationFrame(() => { drawQueued = false; draw(); });
}

/// Called after every frame the editor draws. Cheap when nothing changed: one
/// small status read.
export function customFunctionsTick() {
  const st = readStatus();
  if (!st || cf.registering) return;
  const key = `${st.instance}:${st.generation}`;
  if (key !== cf.seen) {
    const firstSight = !cf.seen || !cf.seen.startsWith(`${st.instance}:`);
    cf.seen = key;
    if (firstSight) {
      cf.epoch++;
      stopWorker();
      cf.errors = [];
      // A workbook that arrives with no functions has nothing to distrust:
      // whatever is written into it later is written here, or by a peer
      // into the same sandbox.
      if (!librarySources().length) cf.trusted.add(st.instance);
    }
    if (!mayRunCustomFunctions()) return;
    if (cf.trusted.has(st.instance)) {
      queueMicrotask(register);
      return;
    }
    if (librarySources().length && !cf.declined.has(st.instance)) offer(st.instance);
    return;
  }
  if (st.pending > 0 && st.registered > 0 && !cf.pumping && cf.trusted.has(st.instance) && mayRunCustomFunctions()) {
    queueMicrotask(pump);
  }
}

/// The person wrote or edited custom functions here: trust this workbook's.
export function trustCustomFunctions() {
  const st = readStatus();
  if (!st) return;
  cf.trusted.add(st.instance);
  cf.declined.delete(st.instance);
  hideOffer();
  cf.seen = null;
  customFunctionsTick();
}

/// The registration's problems, for the Macros window: `[{guid, message, line}]`.
export function customFunctionErrors() {
  return cf.errors.slice();
}

/// The registered functions: `[{name, description, params, returns, …}]`.
export function customFunctionInfo() {
  try { return JSON.parse(wasm.session_custom_functions()); } catch { return []; }
}

// ---- The offer --------------------------------------------------------------

function offerBar() { return byId("oc-cf-bar"); }

function hideOffer() {
  const b = offerBar();
  if (b) { b.hidden = true; b.replaceChildren(); }
}

function offer(instance) {
  const b = offerBar();
  if (!b) return;
  const row = el("div", "oc-recovery-row");
  row.appendChild(el("strong", null, "Custom functions"));
  row.appendChild(el("span", "oc-recovery-detail", " — This workbook defines its own functions. Cells using them show their saved values until you enable them."));
  const actions = el("span", "oc-recovery-actions");
  const yes = el("button", "oc-btn primary", "Enable");
  yes.id = "oc-cf-bar-enable";
  const no = el("button", "oc-btn", "Not now");
  no.id = "oc-cf-bar-later";
  actions.append(yes, no);
  row.appendChild(actions);
  b.replaceChildren(row);
  b.hidden = false;
  yes.addEventListener("click", () => {
    hideOffer();
    cf.trusted.add(instance);
    if (readStatus()?.instance === instance) {
      cf.seen = null;
      customFunctionsTick();
    }
  });
  no.addEventListener("click", () => {
    hideOffer();
    cf.declined.add(instance);
  });
}

// ---- Test hooks -------------------------------------------------------------

/// Shorten the per-call limit, as a test of the cut-off needs.
export function setCustomFunctionLimitForTest(ms) {
  cf.limitMs = Number(ms) > 0 ? Number(ms) : CUSTOM_FUNCTION_TIME_LIMIT_MS;
}

/// Resolves once nothing is registering, pumping or waiting.
export async function customFunctionsSettledForTest(timeoutMs = 20_000) {
  const until = Date.now() + timeoutMs;
  for (;;) {
    customFunctionsTick();
    const st = readStatus();
    const idle = !cf.registering && !cf.pumping && st && st.pending === 0 && st.inFlight === 0;
    if (idle) return true;
    if (Date.now() > until) return false;
    await new Promise((r) => setTimeout(r, 25));
  }
}
