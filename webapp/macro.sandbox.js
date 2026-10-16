// The sandbox both script workers share (`MACRO-01`, `MACRO-04`; docs/99 §5).
//
// Moved out of `macro.worker.js` unchanged when custom functions arrived, so
// the worker that runs a macro and the one that runs a workbook's custom
// functions close exactly the same doors — one list, one lockdown, nothing for
// the two to drift apart over.
//
// Call `lockdown()` after the code to be run has been compiled (the `Function`
// constructor is one of the doors it closes) and before any of it runs. A
// worker that still needs to talk to the page takes `postMessage` and
// `addEventListener` into locals first: the globals are closed here.

import { MacroError } from "./macro.api.js";

/// Every global through which a macro could reach the network, persistent
/// storage, another context or the DOM. Names the worker does not have
/// (`document`, `window`, `importScripts` in a module worker) are defined
/// anyway, so touching one is a named refusal rather than a ReferenceError.
const BLOCKED = [
  "fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "WebTransport",
  "Worker", "SharedWorker", "BroadcastChannel", "indexedDB", "caches", "navigator",
  "document", "window", "parent", "top", "opener", "location", "RTCPeerConnection",
  "postMessage", "onmessage", "close", "eval", "Function", "FileReaderSync", "Request",
  "Response",
];

function violation(name) {
  return function blocked() {
    throw new MacroError("SandboxViolation", `${name} is not available to macros`);
  };
}

export function lockdown() {
  const chain = [];
  for (let o = self; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) chain.push(o);
  for (const name of BLOCKED) {
    const get = violation(name);
    for (const o of chain) {
      if (!Object.prototype.hasOwnProperty.call(o, name)) continue;
      Object.defineProperty(o, name, { get, set: get, configurable: false });
    }
    if (!(name in self)) Object.defineProperty(self, name, { get, set: get, configurable: false });
  }
  // `(function(){}).constructor("…")` is `Function` by another name, and so on
  // for the async and generator kinds.
  for (const f of [function () {}, async function () {}, function* () {}, async function* () {}]) {
    const proto = Object.getPrototypeOf(f);
    Object.defineProperty(proto, "constructor", { get: violation("Function"), configurable: false });
  }
  // A string handed to a timer is evaluated as code.
  for (const name of ["setTimeout", "setInterval"]) {
    const real = self[name].bind(self);
    Object.defineProperty(self, name, {
      value: (fn, ...rest) => {
        if (typeof fn !== "function") throw new MacroError("SandboxViolation", `${name} with a string is not available to macros`);
        return real(fn, ...rest);
      },
      configurable: false,
      writable: false,
    });
  }
  // Fail closed: if any door is still open, nothing runs.
  for (const name of ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts"]) {
    let open = false;
    try { open = typeof self[name] === "function"; } catch { open = false; }
    if (open) throw new MacroError("SandboxError", `could not close ${name}`);
  }
}

