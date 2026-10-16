// Running a macro (`MACRO-01`, docs/99): the editor's half of the engine.
//
// The macro itself runs in `macro.worker.js`, against a private copy of the
// workbook, and comes back as a list of engine calls. This module does the two
// things only the page can do:
//
// * **Bound it.** The worker is started per run and terminated on overrun —
//   from here, where nothing the macro does can stop it. The clock starts when
//   the worker says user code is about to run, so the wasm instantiation and
//   snapshot load are not charged to the macro.
// * **Apply it as one step.** The list is replayed against the real session
//   through the same `session_*` calls the editor's own commands make, under
//   one undo mark, then folded with `session_group_undo_since` — the mechanism
//   typed entry uses to fold a value and the column widening it caused
//   (`XL-UX-036`). Collaborators receive each call as the ordinary operation it
//   is; only the local history is regrouped. If any call fails the whole run is
//   taken back, so a run is all or nothing.
//
// No UI here (the Macros window is `MACRO-02`). `runMacro` is the entry point a
// window, a command or a host will call.

import { REPLAYABLE } from "./macro.api.js";
import {
  BUILD,
  draw,
  effectiveRange,
  invalidateGrowth,
  refreshFormulaBar,
  renderTabs,
  select,
  extend,
  state,
  switchSheet,
  wasm,
} from "./editor.core.js";
import { excelSerialNow, syncVolatileClock } from "./editor.selection.js";
import { CallClock, serveNetwork } from "./editor.network.js";

/// docs/99 §5: a time limit per run. Generous for anything a person would
/// write; a loop that never ends is stopped well before the tab feels hung.
export const MACRO_TIME_LIMIT_MS = 5_000;
/// The most engine calls one run may make (each cell written is one).
export const MACRO_MAX_OPS = 100_000;
/// How long the worker may take to start before user code runs at all.
const START_LIMIT_MS = 30_000;

/// The engine compiled once and handed to every run's worker, so a run pays an
/// instantiation rather than a compile of the whole binary.
let compiled = null;
export function engineModule() {
  compiled ??= WebAssembly.compileStreaming(
    fetch(new URL(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`, import.meta.url)),
  ).catch((e) => {
    compiled = null;
    throw e;
  });
  return compiled;
}

let running = false;

/// The worker's half: run `source` in a fresh sandbox and resolve with what
/// it asked for. Never touches the workbook.
function runInWorker(source, { timeLimitMs, maxOps, name, guid }) {
  return new Promise((resolve) => {
    let worker;
    let timer = null;
    let clock = null;
    let stopNetwork = null;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clock?.stop();
      try { stopNetwork?.(); } catch {}
      try { worker?.terminate(); } catch {}
      resolve(result);
    };
    const fail = (name, message) => finish({ ok: false, error: { name, message: `${name}: ${message}` }, operations: [], ui: [] });
    (async () => {
      let module;
      try { module = await engineModule(); } catch { module = undefined; }
      let snapshot;
      try {
        snapshot = wasm.session_snapshot();
      } catch (e) {
        fail("MacroError", `could not read the workbook: ${e?.message ?? e}`);
        return;
      }
      const sel = effectiveRange();
      const selection = {
        sheet: state.sheet,
        r0: sel.r0, c0: sel.c0, r1: sel.r1, c1: sel.c1,
        row: state.sel.row, col: state.sel.col,
      };
      worker = new Worker(new URL(`./macro.worker.js?b=${BUILD}`, import.meta.url), { type: "module" });
      worker.onerror = (e) => {
        e.preventDefault?.();
        fail("MacroError", e.message || "the macro worker failed");
      };
      // What the macro calls `fetch` lands here (`MACRO-13`, docs/99 §5a).
      const label = String(name || "Macro");
      stopNetwork = serveNetwork(worker, {
        kind: "macro",
        guid: String(guid ?? ""),
        sources: [{ guid: String(guid ?? ""), name: label, source }],
        label: (asker) => (asker === "macro" ? (label.length > 120 ? `${label.slice(0, 117)}…` : label) : null),
        clock: () => clock,
      });
      worker.onmessage = (ev) => {
        const m = ev.data || {};
        if (m.type === "started") {
          clearTimeout(timer);
          // Stopped while the macro waits on the page for a request: the
          // person answering the Security Notice is not the macro running.
          clock = new CallClock(() => fail("TimeLimitExceeded", `the macro ran longer than ${timeLimitMs} ms and was stopped`));
          clock.arm(timeLimitMs);
        } else if (m.type === "done") {
          finish(m);
        }
      };
      timer = setTimeout(() => fail("MacroError", "the macro worker did not start"), START_LIMIT_MS);
      const buf = snapshot.buffer.slice(snapshot.byteOffset, snapshot.byteOffset + snapshot.byteLength);
      worker.postMessage(
        {
          source,
          snapshot: buf,
          module,
          selection,
          clock: { now: excelSerialNow(), seed: Date.now() % 9007199254740991 },
          maxOps,
        },
        [buf],
      );
    })();
  });
}

/// An operation list is data that came out of a sandbox: only known engine
/// calls, only primitive arguments, never more than the bound.
function validOperations(ops, maxOps) {
  if (!Array.isArray(ops) || ops.length > maxOps) return false;
  return ops.every(
    (op) =>
      op && REPLAYABLE.has(op.fn) && Array.isArray(op.args) &&
      op.args.every((a) => ["string", "number", "boolean"].includes(typeof a)),
  );
}

/// Replay `ops` against the live session as one undo step. Returns `null` on
/// success or the error that made it roll back.
function applyOperations(ops) {
  const mark = wasm.session_undo_mark();
  syncVolatileClock();
  let failure = null;
  for (const op of ops) {
    try {
      wasm[op.fn](...op.args);
    } catch (e) {
      failure = { name: "ReplayError", message: `ReplayError: ${op.fn} was refused: ${e?.message ?? e}` };
      break;
    }
  }
  const moved = wasm.session_undo_mark() > mark;
  const grouped = moved ? wasm.session_group_undo_since(mark) : true;
  if (failure && moved) {
    // All or nothing: the copy accepted every call, so a refusal here means
    // the workbook moved under the run. Take back what was applied.
    if (grouped) {
      try { wasm.session_undo(); } catch {}
    }
  }
  if (!failure && !grouped) {
    failure = { name: "UndoGroupError", message: "UndoGroupError: the run applied but could not be made one undo step" };
  }
  return failure;
}

/// The editor-only steps a macro asked for (the active sheet, the selection),
/// after its edits — they are not edits, so they are not in the undo step.
function applyUi(ui) {
  for (const step of ui) {
    try {
      if (step.op === "activate" || step.op === "select") {
        if (Number.isInteger(step.sheet) && step.sheet !== state.sheet) switchSheet(step.sheet);
      }
      if (step.op === "select") {
        select(step.r0, step.c0);
        if (step.r1 !== step.r0 || step.c1 !== step.c0) extend(step.r1, step.c1);
      }
    } catch { /* a stale sheet index after a delete is not worth failing the run */ }
  }
}

/// Run a macro against the open workbook.
///
/// Resolves `{ ok, error, operations }`: `operations` is the engine calls
/// applied, as `{ fn, args }` (empty whenever `ok` is false — a failed run
/// changes nothing), and
/// `error` is `{ name, message, line }` — `NotSupported`, `SandboxViolation`,
/// `TimeLimitExceeded`, `LimitExceeded`, or whatever the macro threw.
export async function runMacro(source, opts = {}) {
  const timeLimitMs = Number(opts.timeLimitMs) > 0 ? Number(opts.timeLimitMs) : MACRO_TIME_LIMIT_MS;
  const maxOps = Number(opts.maxOps) > 0 ? Number(opts.maxOps) : MACRO_MAX_OPS;
  if (running) return { ok: false, error: { name: "MacroBusy", message: "MacroBusy: a macro is already running" }, operations: [] };
  // The worker's copy is loaded from a snapshot, which carries the workbook but
  // not this session's read-only mode: the copy would accept every write and
  // the replay would then be refused. Asked here, before anything runs.
  let readOnly = false;
  try { readOnly = !!wasm.session_read_only(); } catch {}
  if (readOnly) return { ok: false, error: { name: "ReadOnly", message: "ReadOnly: this workbook is open read-only" }, operations: [] };
  running = true;
  try {
    const res = await runInWorker(String(source ?? ""), { timeLimitMs, maxOps, name: opts.name, guid: opts.guid });
    if (!res.ok) return { ok: false, error: res.error, operations: [] };
    if (!validOperations(res.operations, maxOps)) {
      return { ok: false, error: { name: "MacroError", message: "MacroError: the macro returned an invalid operation list" }, operations: [] };
    }
    const failure = res.operations.length ? applyOperations(res.operations) : null;
    invalidateGrowth();
    renderTabs();
    if (failure) {
      draw();
      return { ok: false, error: failure, operations: [] };
    }
    applyUi(Array.isArray(res.ui) ? res.ui : []);
    draw();
    try { refreshFormulaBar(); } catch {}
    return { ok: true, error: null, operations: res.operations };
  } finally {
    running = false;
  }
}

/// The hook the browser suite drives (`window.opencalcEditor.runMacroForTest`).
/// The same path as `runMacro`; named for what it is for.
export function runMacroForTest(source, opts) {
  return runMacro(source, opts);
}
