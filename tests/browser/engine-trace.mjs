// Record what an editor asks its engine, and what the engine answers.
//
// `TAURI-003` asks how a desktop webview reaches the engine, and the option
// that keeps both native calc and one editor — a swappable binding — has a risk
// no diagram shows: two implementations of 229 calls that must agree, where a
// divergence stays invisible until a user finds it.
//
// That risk is only acceptable with a way to check it, so this is the check.
// It wraps the single `wasm` handle the editor already funnels everything
// through, drives a scripted session, and writes down every call and every
// answer. Run the same script against a Tauri command bridge and the two traces
// must match — the same shape `oracle-diff` uses to compare this engine against
// LibreOffice.
//
// It is useful whichever option `TAURI-003` takes: a trace of what an editor
// actually calls is what tells you how big a second implementation would be,
// and which calls matter most.

/// Wrap `api` so every call is recorded, then return `[proxy, trace]`.
///
/// A `Proxy` rather than 229 hand-written wrappers, for the reason `UX-SITE-03`
/// makes the hard way: a rule that has to enumerate its subjects is one
/// omission away from being wrong, and this one would need re-enumerating every
/// time the engine gained a call.
export function recording(api) {
  const trace = [];
  const proxy = new Proxy(api, {
    get(target, name) {
      const value = target[name];
      if (typeof value !== "function") return value;
      return (...args) => {
        let answer, threw = null;
        try {
          answer = value.apply(target, args);
        } catch (e) {
          threw = String(e?.message ?? e);
        }
        trace.push({
          call: String(name),
          args: args.map(summarise),
          // Answers are summarised, not stored whole: a `session_cells` reply
          // is tens of kilobytes, and a trace nobody can read is a trace nobody
          // checks. The summary still changes when the answer does.
          answer: summarise(answer),
          threw,
        });
        if (threw !== null) throw new Error(threw);
        return answer;
      };
    },
  });
  return [proxy, trace];
}

/// A value as a trace entry: short things verbatim, long things by shape.
///
/// Deliberately not a hash. Two engines disagreeing should produce a difference
/// somebody can *read* — `len 4821 "[{\"r\":0` versus `len 4103 "[{\"r\":0` says
/// where to look, and `a3f9…` versus `71c2…` says only that you are in trouble.
function summarise(v) {
  if (v === undefined) return "undefined";
  if (v === null) return "null";
  if (typeof v === "string") {
    return v.length <= 80 ? JSON.stringify(v) : `len ${v.length} ${JSON.stringify(v.slice(0, 40))}`;
  }
  if (typeof v === "object") return `object ${Object.keys(v).length} keys`;
  return String(v);
}

/// The session every conformance run drives.
///
/// One script, so two engines are asked exactly the same questions in exactly
/// the same order. It touches the paths a divergence would actually hurt in:
/// typing, formulas, structure, formatting and undo — not a survey of all 229,
/// which would be a slower test that proves less.
export const SCRIPT = [
  ["session_new"],
  ["session_set_cell", 0, 0, 0, "Item"],
  ["session_set_cell", 0, 1, 0, "12"],
  ["session_set_cell", 0, 2, 0, "007"],
  ["session_set_cell", 0, 3, 0, "'0123"],
  ["session_set_cell", 0, 1, 1, "=A2*2"],
  ["session_cell_input", 0, 1, 1],
  ["session_cell_format", 0, 2, 0],
  ["session_insert_rows", 0, 1, 1],
  ["session_cell_input", 0, 2, 1],
  ["session_set_col_width", 0, 0, 140],
  ["session_col_px", 0, 0, 3],
  ["session_toggle_bold", 0, 0, 0, 0, 0],
  ["session_cell_format", 0, 0, 0],
  ["session_undo"],
  ["session_cell_format", 0, 0, 0],
  ["session_cells", 0, 0, 0, 6, 3],
  ["session_sheet_names"],
];

/// Drive `SCRIPT` against a recording proxy and return the trace.
export function runScript(api) {
  const [proxy, trace] = recording(api);
  for (const [name, ...args] of SCRIPT) {
    try {
      proxy[name](...args);
    } catch {
      // Recorded by the proxy. A call that refuses is part of the contract —
      // two engines must refuse the same things — so the run continues.
    }
  }
  return trace;
}
