// The macro sandbox (`MACRO-01`, docs/99 §5): one dedicated worker per run.
//
// ## Why the worker holds a copy of the workbook (the read protocol)
//
// A macro reads as well as writes, and reads must see its own writes —
// `GetRange("A1").SetValue(2)` then `GetRange("B1").GetValue()` where B1 is
// `=A1*2` must answer 4. Three ways to answer a read synchronously were weighed:
//
// 1. **Request/response over `Atomics.wait`** on a `SharedArrayBuffer`. Exact,
//    but `SharedArrayBuffer` exists only on a cross-origin-isolated page, which
//    needs COOP/COEP response headers. The static Pages host cannot send them,
//    and an embedding host would have to as well. Ruled out: it does not run
//    where the editor runs.
// 2. **A snapshot of the cells the macro reads**, pre-read on the main thread.
//    Which cells is not knowable before the code runs, and a pre-read cannot
//    follow the macro's own writes through a formula without re-implementing
//    recalculation in JavaScript — a second engine, which docs/99 forbids.
// 3. **The engine itself, in the worker, over a snapshot of the workbook.**
//    The editor hands the worker `session_snapshot()` (the collaboration join
//    format, the whole model) and the compiled wasm module. The macro's `Api`
//    calls run against that private copy, so every read is synchronous and
//    exact, recalculated by the same engine; every mutation the copy accepted
//    is also recorded by name and arguments. The editor replays that list
//    against the real workbook through the same `session_*` calls, as one
//    undo step.
//
// (3) is the one chosen. It works on any static host, needs no new engine
// semantics (the replay *is* the editor's own call path), and keeps the
// guarantee docs/99 asks for: the real workbook is never touched until the
// run has finished cleanly, so a macro that throws, overruns or is killed
// changes nothing. Its cost is a wasm instantiation and a snapshot per run,
// paid before the clock starts.
//
// ## The sandbox
//
// Before user code runs, every network and storage door the worker has is
// replaced by a getter that throws `SandboxViolation` — on the global *and* on
// each prototype it inherits from, since `WorkerGlobalScope.prototype.fetch`
// is reachable from user code. String-to-code paths (`eval`, the `Function`
// constructors) are closed too, and source that mentions `import` is refused
// before it is compiled, because `import()` is syntax, not a global, and no
// lockdown of globals can reach it. The time limit is enforced from outside:
// the editor terminates the worker, which nothing inside it can prevent.

import { MacroError, createApi } from "./macro.api.js";
import { lockdown } from "./macro.sandbox.js";

const BUILD =
  new URL(import.meta.url).searchParams.get("b") ||
  new URL(import.meta.url).searchParams.get("v") ||
  "dev";

/// Taken before user code exists; the global is closed below.
const post = self.postMessage.bind(self);

/// The source as a function of `Api`. Not strict, because macros written for
/// ONLYOFFICE are not and some lean on that.
function compile(source) {
  if (/\bimport\b/.test(source)) {
    throw new MacroError("SandboxViolation", "import is not available to macros (remove the word `import`)");
  }
  // eslint-disable-next-line no-new-func
  return new Function("Api", source);
}

/// Where in the macro an error was thrown: `new Function` puts two header
/// lines before the body.
function lineOf(err) {
  const m = /<anonymous>:(\d+):(\d+)/.exec(String(err && err.stack));
  return m ? Math.max(1, Number(m[1]) - 2) : null;
}

self.onmessage = async (ev) => {
  // One run per worker; a second message is not a second macro.
  self.onmessage = null;
  const { source, snapshot, module, selection, clock, maxOps } = ev.data || {};
  const operations = [];
  const ui = [];
  try {
    const glue = await import(`./pkg/casual_calc_wasm.js?b=${BUILD}`);
    await glue.default({
      module_or_path: module ?? new URL(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`, import.meta.url),
    });
    glue.session_load_snapshot(new Uint8Array(snapshot));
    if (clock) glue.session_set_volatile(clock.now, clock.seed);
    const limit = Number(maxOps) > 0 ? Number(maxOps) : 100_000;
    const ctx = {
      engine: glue,
      selection,
      record(fn, args) {
        if (operations.length >= limit) {
          throw new MacroError("LimitExceeded", `more than ${limit} operations in one run`);
        }
        operations.push({ fn, args });
      },
      ui(op) { ui.push(op); },
    };
    const Api = createApi(ctx);
    const fn = compile(String(source ?? ""));
    lockdown();
    post({ type: "started" });
    const out = fn.call(undefined, Api);
    if (out && typeof out.then === "function") await out;
    post({ type: "done", ok: true, operations, ui, selection: ctx.selection });
  } catch (err) {
    const name = (err && err.name) || "Error";
    const message = err && err.message ? String(err.message) : String(err);
    post({ type: "done", ok: false, error: { name, message, line: lineOf(err) }, operations: [], ui: [] });
  }
};
