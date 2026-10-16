// Open and save off the page's thread (`PERF-16`): the editor's half.
//
// Open took ~0.3 s and save ~0.3 s for a 250,000-cell workbook (measured in
// Chromium, more on a slower machine) as single calls on the thread that draws
// the grid, so the editor froze for as long as either ran. Both now run in
// `file.worker.js`, and what is left on this thread is reading or writing the
// workbook's transfer parts a few milliseconds at a time, yielding between
// them so frames, scrolling and input go on.
//
// What stays the same is everything the open and the save *decide*: the
// worker opens with the same `session_open_as` under the same admission
// bounds and time budget; the page installs the result through
// `set_session`, so the host's read-only setting (`SEC-025`) and clock apply
// exactly as on the main-thread path; and every caller keeps its bookkeeping
// (import report, macros bar, drafts, save target) — this module only replaces
// the one long engine call each of them made.
//
// **Excel's progress, Excel's cancel.** While a job runs the status bar says
// "Opening 'name'" or "Saving 'name'" beside a progress bar, and Esc stops it.
// A stopped open leaves the workbook on screen as it was (nothing is installed
// until the last part is read); a stopped save writes nothing (the bytes never
// reach a download or a save target).
//
// **Fallback.** No `Worker`, a worker script or engine that will not load, or
// a worker that dies before it is ready: the job reports `unavailable` and the
// caller runs the main-thread path it always had. Only a failure *before* the
// worker is ready falls back — one after it is a real answer about the file.

import { BUILD, byId, wasm } from "./editor.core.js";
import { engineModule } from "./editor.macros.js";

/// Formulas or cells per transfer part. Reading one part of this size on the
/// page's thread costs a few milliseconds.
export const PART_ITEMS = 4096;
/// How long one slice of page-thread work may run before it yields.
const SLICE_MS = 8;
/// How long the worker may take to load the engine before the job falls back.
const BOOT_LIMIT_MS = 20_000;

let forceMainThread = false;
let active = null;

/// For tests: run every open and save on the page's thread, as before this
/// module existed.
export function setFileWorkersForTest(on) {
  forceMainThread = !on;
}

/// Whether a file job is running.
export function fileJobActive() {
  return active !== null;
}

/// Stop the running job, if any. What Esc does.
export function cancelFileJob() {
  if (active) active.cancel();
}

/// Yield to the event loop without the 1 s clamp a background tab puts on
/// `setTimeout`, so a job started in a tab that is then hidden still finishes.
export function yieldTask() {
  return new Promise((resolve) => {
    const ch = new MessageChannel();
    ch.port1.onmessage = () => resolve();
    ch.port2.postMessage(0);
  });
}

/// Yield until a frame has been drawn: two steps that must not land in one
/// frame are separated by this rather than by [`yieldTask`], which lets the
/// browser run the next task before it paints. Bounded, because a hidden tab
/// draws no frames at all.
export function yieldFrame() {
  return new Promise((resolve) => {
    let done = false;
    const go = () => { if (!done) { done = true; resolve(); } };
    requestAnimationFrame(() => yieldTask().then(go));
    setTimeout(go, 100);
  });
}

// --- The status-bar progress -------------------------------------------------

function progressEl() {
  let el = byId("file-progress");
  if (el) return el;
  const bar = byId("tb-status");
  if (!bar || !bar.parentNode) return null;
  el = document.createElement("span");
  el.id = "file-progress";
  el.className = "file-progress";
  el.setAttribute("role", "progressbar");
  el.setAttribute("aria-valuemin", "0");
  el.setAttribute("aria-valuemax", "100");
  const fill = document.createElement("span");
  fill.className = "file-progress-fill";
  el.append(fill);
  bar.parentNode.insertBefore(el, bar.nextSibling);
  return el;
}

function showProgress(label, fraction) {
  const status = byId("tb-status");
  if (status && status.textContent !== label) status.textContent = label;
  const el = progressEl();
  if (!el) return;
  el.setAttribute("aria-label", label);
  const fill = el.firstChild;
  if (fraction === null) {
    el.classList.add("indeterminate");
    el.removeAttribute("aria-valuenow");
    fill.style.width = "";
  } else {
    const pct = Math.max(0, Math.min(100, Math.round(fraction * 100)));
    el.classList.remove("indeterminate");
    el.setAttribute("aria-valuenow", String(pct));
    fill.style.width = `${pct}%`;
  }
}

function hideProgress() {
  byId("file-progress")?.remove();
}

/// Esc stops the job, ahead of anything else Esc would do: while a file is
/// being opened or saved, that is what the key means (Office: "Press the ESC
/// key to cancel").
function onKey(e) {
  if (e.key !== "Escape" || !active) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  cancelFileJob();
}

// --- The job ----------------------------------------------------------------

/// Start a worker and wait for its engine. Resolves with the worker, or
/// `null` when workers cannot be used here.
async function startWorker(job) {
  if (forceMainThread || typeof Worker === "undefined") return null;
  let module;
  try { module = await engineModule(); } catch { module = undefined; }
  if (job.cancelled) return null;
  let worker;
  try {
    worker = new Worker(new URL(`./file.worker.js?b=${BUILD}`, import.meta.url), { type: "module" });
  } catch {
    return null;
  }
  job.worker = worker;
  const ready = await new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), BOOT_LIMIT_MS);
    const done = (ok) => { clearTimeout(timer); resolve(ok); };
    job.onCancel = () => done(false);
    worker.onerror = (e) => { e.preventDefault?.(); done(false); };
    worker.onmessage = (ev) => done(ev.data?.type === "ready");
    worker.postMessage({ type: "boot", module });
  });
  job.onCancel = null;
  if (!ready) {
    try { worker.terminate(); } catch {}
    job.worker = null;
    return null;
  }
  return worker;
}

function beginJob() {
  if (active) return null;
  const job = {
    cancelled: false,
    worker: null,
    onCancel: null,
    cancel() {
      if (this.cancelled) return;
      this.cancelled = true;
      try { this.worker?.terminate(); } catch {}
      this.onCancel?.();
    },
  };
  active = job;
  document.addEventListener("keydown", onKey, true);
  return job;
}

function endJob(job) {
  if (active === job) active = null;
  document.removeEventListener("keydown", onKey, true);
  hideProgress();
}

/// Open `bytes` (already decoded, for text formats) as `ext` in a worker and
/// read the result into the page's engine, **without installing it**.
///
/// Resolves with one of:
///
/// - `{ outcome: "ready", install() }` — every part is read; `install()`
///   replaces the session (and is the only thing that does).
/// - `{ outcome: "error", error }` — the worker's answer about the file.
/// - `{ outcome: "cancelled" }` — Esc; nothing changed.
/// - `{ outcome: "unavailable" }` — no worker here; use the main-thread path.
/// - `{ outcome: "busy" }` — another open or save is running.
export async function openInWorker(bytes, ext, name, budgetMs, clock) {
  const job = beginJob();
  if (!job) return { outcome: "busy" };
  const label = `Opening '${name}'`;
  try {
    showProgress(label, null);
    const worker = await startWorker(job);
    if (job.cancelled) return { outcome: "cancelled" };
    if (!worker) return { outcome: "unavailable" };
    wasm.session_receive_begin();
    const result = await new Promise((resolve) => {
      const queue = [];
      let total = 0;
      let report = "[]";
      let finished = false;
      let pumping = false;
      let failed = null;
      job.onCancel = () => resolve({ outcome: "cancelled" });
      const settle = () => {
        if (failed) resolve({ outcome: "error", error: failed });
        else if (finished && !queue.length) resolve({ outcome: "ready", report });
      };
      const pump = async () => {
        if (pumping) return;
        pumping = true;
        while (queue.length && !job.cancelled && !failed) {
          const until = performance.now() + SLICE_MS;
          let received = 0;
          while (queue.length && performance.now() < until) {
            try {
              received = wasm.session_receive_part(queue.shift());
            } catch (e) {
              failed = e;
              queue.length = 0;
              break;
            }
          }
          if (total > 0 && received) showProgress(label, received / total);
          await yieldTask();
        }
        pumping = false;
        if (!job.cancelled) settle();
      };
      worker.onerror = (e) => {
        e.preventDefault?.();
        failed = new Error(e.message || "the file worker failed");
        settle();
      };
      worker.onmessage = (ev) => {
        const m = ev.data || {};
        if (m.type === "opened") {
          total = m.total;
          report = m.report;
          showProgress(label, 0);
        } else if (m.type === "part") {
          queue.push(new Uint8Array(m.bytes));
          pump();
        } else if (m.type === "done") {
          finished = true;
          if (!pumping) settle();
        } else if (m.type === "error") {
          failed = new Error(m.message);
          settle();
        }
      };
      // A copy goes to the worker: the page keeps the original as the
      // session's source, so an unedited save hands it back unchanged.
      const copy = bytes.slice().buffer;
      worker.postMessage({ type: "open", ext, bytes: copy, budgetMs, clock, partItems: PART_ITEMS }, [copy]);
    });
    if (result.outcome !== "ready") {
      try { wasm.session_receive_cancel(); } catch {}
      return result;
    }
    return {
      outcome: "ready",
      install() {
        wasm.session_receive_finish_opened(ext, result.report, bytes);
      },
      discard() {
        try { wasm.session_receive_cancel(); } catch {}
      },
    };
  } finally {
    try { job.worker?.terminate(); } catch {}
    endJob(job);
  }
}

/// Write the workbook in a worker.
///
/// `what` is `{ ext }` for a package or `.ods` (`session_save_as(ext)`), or
/// `{ delim, sheet, ext }` for delimited text (`session_save_delimited`).
/// `format` is the session's own format, so the worker's copy is the same kind
/// of session. The workbook is copied the moment this is called: edits made
/// while it runs are not in the file.
///
/// Resolves with `{ outcome: "ok", bytes | text }`, `"error"`, `"cancelled"`
/// (nothing written), `"unavailable"` or `"busy"` as for [`openInWorker`].
export async function saveInWorker(what, name) {
  const job = beginJob();
  if (!job) return { outcome: "busy" };
  const label = `Saving '${name}'`;
  try {
    showProgress(label, null);
    const worker = await startWorker(job);
    if (job.cancelled) return { outcome: "cancelled" };
    if (!worker) return { outcome: "unavailable" };
    // After the worker is up, so a fallback has not paid for a copy.
    const total = wasm.session_transfer_begin();
    const format = wasm.session_format();
    return await new Promise((resolve) => {
      job.onCancel = () => resolve({ outcome: "cancelled" });
      worker.onerror = (e) => {
        e.preventDefault?.();
        resolve({ outcome: "error", error: new Error(e.message || "the file worker failed") });
      };
      worker.onmessage = (ev) => {
        const m = ev.data || {};
        if (m.type === "bytes") resolve({ outcome: "ok", bytes: new Uint8Array(m.bytes) });
        else if (m.type === "text") resolve({ outcome: "ok", text: m.text });
        else if (m.type === "error") resolve({ outcome: "error", error: new Error(m.message) });
      };
      worker.postMessage({ type: "receive" });
      (async () => {
        for (;;) {
          if (job.cancelled) return;
          const until = performance.now() + SLICE_MS;
          let last = false;
          while (performance.now() < until) {
            const part = wasm.session_transfer_next(PART_ITEMS);
            if (!part.length) { last = true; break; }
            const buf = part.buffer.slice(part.byteOffset, part.byteOffset + part.byteLength);
            worker.postMessage({ type: "part", bytes: buf }, [buf]);
          }
          if (last) break;
          if (total > 0) showProgress(label, wasm.session_transfer_sent() / total);
          await yieldTask();
        }
        showProgress(label, null);
        worker.postMessage({ type: "write", ext: what.ext, format, delim: what.delim || 0, sheet: what.sheet || 0 });
      })().catch((e) => resolve({ outcome: "error", error: e }));
    });
  } finally {
    try { wasm.session_transfer_cancel(); } catch {}
    try { job.worker?.terminate(); } catch {}
    endJob(job);
  }
}
