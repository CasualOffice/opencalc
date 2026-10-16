// What a macro or custom function calls `fetch` (`MACRO-13`, docs/99 §5a).
//
// **Not a network primitive.** The sandbox (`macro.sandbox.js`) still removes
// `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`, `importScripts` and
// `navigator` from the worker, and this module never touches any of them. The
// stub writes the request down as plain data and posts it to the page; the
// page decides whether to make it, asks the person, makes it with options it
// fixes itself, and posts back a bounded copy of the reply. So no lockdown
// escape inside the worker turns into network access: there is nothing here
// to escape *to*.
//
// The stub is handed to the user's source as a parameter called `fetch`
// (`new Function("Api", "fetch", source)`), so a script's plain `fetch(…)`
// resolves to it lexically. The global `fetch` stays a refused door — a
// script that goes looking for the real one (`self.fetch`, the prototype
// chain, `Function("return this")()`) finds `SandboxViolation`, not this.
//
// Each source gets its own stub, which names it to the page (`asker`); the
// page does not take the worker's word for anything else.

import { MacroError } from "./macro.api.js";

/// `{ post, listen }` are the worker's `postMessage` and `addEventListener`,
/// taken before the lockdown. Returns `stubFor(asker)`.
export function networkStubs(post, listen) {
  const waiting = new Map();
  let next = 1;
  let quiet = [];
  listen("message", (ev) => {
    const m = ev.data;
    if (!m || m.type !== "net-reply") return;
    const w = waiting.get(m.id);
    if (!w) return;
    waiting.delete(m.id);
    if (m.ok) w.resolve(response(m.status, m.text));
    else w.reject(new MacroError(String(m.error?.name || "NetworkError"), String(m.error?.message || "the request failed")));
    if (waiting.size === 0) {
      const q = quiet;
      quiet = [];
      for (const r of q) r();
    }
  });
  stubFor.pending = () => waiting.size;
  /// Resolves when no request is waiting on the page.
  stubFor.idle = () => (waiting.size === 0 ? Promise.resolve() : new Promise((r) => quiet.push(r)));
  return stubFor;
  function stubFor(asker) {
    const who = String(asker ?? "");
    return function fetch(input, init) {
      // Read everything into primitives here, once: what is posted is plain
      // data, and a getter the script planted on `init` runs now, in the
      // script's own world, not during the structured clone.
      let request;
      try {
        const o = init ?? {};
        const headers = {};
        if (o.headers !== undefined && o.headers !== null) {
          for (const k of Object.keys(o.headers)) headers[String(k)] = String(o.headers[k]);
        }
        request = {
          url: String(input),
          method: o.method === undefined ? "GET" : String(o.method),
          headers,
          body: o.body === undefined || o.body === null ? null : typeof o.body === "string" ? o.body : { notString: true },
        };
      } catch (err) {
        return Promise.reject(new MacroError("NetworkBlocked", `the request could not be read: ${err?.message ?? err}`));
      }
      const id = next++;
      return new Promise((resolve, reject) => {
        waiting.set(id, { resolve, reject });
        post({ type: "net", id, asker: who, request });
      });
    };
  }
}

/// A reply as the script sees it: `status`, `ok`, `text()` and `json()` and
/// nothing else — no headers, no body stream, no URL.
function response(status, text) {
  const body = String(text ?? "");
  const s = Number(status) | 0;
  return Object.freeze({
    status: s,
    ok: s >= 200 && s <= 299,
    text: () => Promise.resolve(body),
    json: () => {
      try { return Promise.resolve(JSON.parse(body)); } catch (err) { return Promise.reject(err); }
    },
  });
}
