// Opening and saving off the page's thread (`PERF-16`): one dedicated worker
// per job, driven by `editor.files.js`.
//
// The worker loads the same engine the page did (same `?b=` build, and the
// page hands over the compiled module so this is an instantiation, not a
// compile). The two engines share nothing — no `SharedArrayBuffer` without
// cross-origin isolation, which the static host cannot send (see
// `macro.worker.js`) — so a workbook crosses as bounded transfer parts
// (`casual_calc_model::transfer`), each read on the receiving side in a few
// milliseconds.
//
// - **open**: `session_open_as` here, under every admission bound and the
//   time budget the page asks for — the same call the main-thread open makes —
//   then the parts out, one message each, buffers transferred.
// - **save**: parts in, `session_receive_finish_copy`, then the ordinary
//   `session_save_as` / `session_save_delimited` writer, bytes transferred back.
//
// A cancelled job is a terminated worker: nothing here writes anywhere, so
// there is nothing to undo.

const BUILD =
  new URL(import.meta.url).searchParams.get("b") ||
  new URL(import.meta.url).searchParams.get("v") ||
  "dev";

let glue = null;

function fail(err) {
  const message = err && err.message ? String(err.message) : String(err);
  self.postMessage({ type: "error", message });
}

function postBytes(type, bytes, extra = {}) {
  // A view into wasm memory is copied out once; the copy is transferred.
  const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  self.postMessage({ type, bytes: buf, ...extra }, [buf]);
}

async function boot(module) {
  const mod = await import(`./pkg/casual_calc_wasm.js?b=${BUILD}`);
  await mod.default({
    module_or_path: module ?? new URL(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`, import.meta.url),
  });
  glue = mod;
}

function open({ ext, bytes, budgetMs, clock, partItems }) {
  glue.session_set_time_budget_ms(Number.isFinite(budgetMs) ? budgetMs : -1);
  try {
    glue.session_open_as(ext, new Uint8Array(bytes));
  } finally {
    glue.session_clear_time_budget();
  }
  // The page would set its clock on the new session next, and that is a full
  // recalculation; it runs here instead, under the page's own two numbers,
  // and the page adopts them without repeating it.
  if (clock) glue.session_set_clock(clock.serial, clock.seed);
  const report = glue.session_opened_report();
  const total = glue.session_transfer_begin();
  self.postMessage({ type: "opened", report, total });
  for (;;) {
    const part = glue.session_transfer_next(partItems);
    if (!part.length) break;
    postBytes("part", part);
  }
  self.postMessage({ type: "done" });
}

function write({ ext, format, delim, sheet }) {
  glue.session_receive_finish_copy(format);
  if (delim) {
    self.postMessage({ type: "text", text: glue.session_save_delimited(sheet, delim) });
  } else {
    postBytes("bytes", glue.session_save_as(ext));
  }
}

self.onmessage = async (ev) => {
  const m = ev.data || {};
  try {
    if (m.type === "boot") {
      await boot(m.module);
      self.postMessage({ type: "ready" });
    } else if (m.type === "open") {
      open(m);
    } else if (m.type === "receive") {
      glue.session_receive_begin();
    } else if (m.type === "part") {
      glue.session_receive_part(new Uint8Array(m.bytes));
    } else if (m.type === "write") {
      write(m);
    }
  } catch (err) {
    fail(err);
  }
};
