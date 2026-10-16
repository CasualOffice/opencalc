// Network access for macros and custom functions, only when the person allows
// it (`MACRO-13`, docs/99 §5a). The page's half.
//
// A script never holds a network primitive. What it calls `fetch` is a stub
// (`macro.net-stub.js`) that posts `{type:"net", id, asker, request}` here.
// This module, in order:
//
// 1. **Host policy.** Nothing without `canRunMacros`, nothing read-only,
//    nothing when the host says `canUseNetwork: false`, and only sites on
//    `networkAllowlist` when the host gives one. A refusal here never prompts.
// 2. **The address and the request** (`macro.net-policy.js`): https only, no
//    IP literals or local names, never our own origin or a collaboration
//    server; GET/HEAD/POST, Accept and Content-Type only, a body up to 1 MB.
// 3. **Rate**: 60 a minute per workbook.
// 4. **Consent**, in Excel's shape: a Security Warning message bar and a
//    Security Notice with Enable and Disable. A grant is for one site, one
//    workbook and the exact sources that asked (SHA-256, computed here from
//    the sources this page loaded into the worker — never the worker's word),
//    and lives on this device only.
// 5. **The request**, made here with options fixed here: `credentials:
//    "omit"`, `referrerPolicy: "no-referrer"`, `cache: "no-store"`, `mode:
//    "cors"`, `redirect: "error"`, 10 s, 4 at once. The page's own CSP applies
//    to it as to any other request and is not worked around.
// 6. **The reply**, read as text and cut off at 5 MB while it streams; only
//    the status and the text go back. The script sees `status`, `ok`,
//    `text()` and `json()`. It reaches cells only as a function's answer or
//    through the macro API — never as HTML, script or a formula.
//
// ## The document id
//
// docs/99 §5a keys a grant by the workbook's document id (XL-FEAT-113). That
// id is not built yet, so this uses the half of XL-FEAT-113's design that
// needs nothing written into a file: an opened file is the SHA-256 of its
// bytes, and a workbook that did not come from bytes (new, joined, recovered)
// gets an id for this tab only. That is narrower than a carried id — a grant
// does not follow a file through a save, so the person is asked again — and
// never wider.

import { NET_LIMITS, checkRequest, checkUrl } from "./macro.net-policy.js";
import { byId, el, getCapabilities, status, wasm } from "./editor.core.js";
import { documentName } from "./editor.sheets.js";

// ---- Host policy -------------------------------------------------------------

/// Collaboration servers this tab has talked to, by origin.
const collabOrigins = new Set();

/// `collaborate()` records its endpoint here, so a workbook cannot reach the
/// collaboration server as the person.
export function noteCollabEndpoint(url) {
  try {
    const u = new URL(String(url), location.href);
    collabOrigins.add(`https://${u.hostname}`);
  } catch {}
}

/// Whether a workbook may use the network at all here, and the host's list.
export function networkPolicy() {
  const caps = getCapabilities();
  if (caps.canRunMacros === false) return { ok: false, reason: "macros are not allowed here" };
  if (caps.readOnly) return { ok: false, reason: "the workbook is open read-only" };
  let readOnly = false;
  try { readOnly = !!wasm.session_read_only(); } catch {}
  if (readOnly) return { ok: false, reason: "the workbook is open read-only" };
  if (caps.canUseNetwork === false) return { ok: false, reason: "this host does not allow network access" };
  return { ok: true, allowlist: Array.isArray(caps.networkAllowlist) ? caps.networkAllowlist : null };
}

// ---- The document id ---------------------------------------------------------

/// Workbook instance → its id (a promise while the digest runs).
const identities = new Map();

function currentInstance() {
  try { return String(JSON.parse(wasm.session_custom_status()).instance); } catch { return "?"; }
}

function hex(buf) {
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function sha256(text) {
  return hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
}

function randomId() {
  try { if (crypto.randomUUID) return crypto.randomUUID(); } catch {}
  return hex(crypto.getRandomValues(new Uint8Array(16)));
}

/// `openBytes` calls this after a successful open: the workbook now on screen
/// is these bytes.
export function noteOpenedBytes(raw) {
  const instance = currentInstance();
  let bytes = null;
  try {
    if (raw instanceof ArrayBuffer) bytes = new Uint8Array(raw.slice(0));
    else if (ArrayBuffer.isView(raw)) bytes = new Uint8Array(raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength));
  } catch { bytes = null; }
  // Not bytes (or none): this tab's own id, never a digest of nothing that
  // every such workbook would share.
  if (!bytes || bytes.byteLength === 0) { identities.set(instance, Promise.resolve(`tab:${randomId()}`)); return; }
  identities.set(
    instance,
    crypto.subtle.digest("SHA-256", bytes).then((d) => `file:${hex(d)}`, () => `tab:${randomId()}`),
  );
}

/// The id a grant for the workbook on screen is kept under.
export async function documentIdentity() {
  const instance = currentInstance();
  if (!identities.has(instance)) identities.set(instance, Promise.resolve(`tab:${randomId()}`));
  return identities.get(instance);
}

// ---- Grants: this device only --------------------------------------------------

const GRANTS_KEY = "opencalc.network-grants.v1";
/// The page's own copy, which is all there is when storage is unavailable
/// (a private window, blocked site data): grants then last for this page.
let memoryGrants = [];

function validGrant(g) {
  return g && ["doc", "site", "hash", "kind", "who"].every((k) => typeof g[k] === "string");
}

function readGrants() {
  try {
    const raw = localStorage.getItem(GRANTS_KEY);
    if (raw !== null) {
      const list = JSON.parse(raw);
      if (Array.isArray(list)) return list.filter(validGrant);
    }
  } catch {}
  return memoryGrants.slice();
}

function writeGrants(list) {
  memoryGrants = list.slice();
  try { localStorage.setItem(GRANTS_KEY, JSON.stringify(list)); } catch {}
}

const grantKey = (doc, site, hash) => `${doc}\n${site}\n${hash}`;

function hasGrant(doc, site, hash) {
  return readGrants().some((g) => g.doc === doc && g.site === site && g.hash === hash);
}

function addGrant(g) {
  const list = readGrants().filter((x) => !(x.doc === g.doc && x.site === g.site && x.hash === g.hash));
  list.push({ ...g, at: new Date().toISOString() });
  writeGrants(list);
}

/// Editing a source invalidates its grants: drop the grants this workbook holds
/// for the same asker (`kind`, and the macro's `guid`) under any other hash.
function pruneGrants(doc, kind, guid, hash) {
  const list = readGrants();
  const kept = list.filter((g) => !(g.doc === doc && g.kind === kind && (g.guid || "") === (guid || "") && g.hash !== hash));
  if (kept.length !== list.length) writeGrants(kept);
}

/// For the Trust Center: `[{doc, site, kind, who, workbook, at}]`.
export function listNetworkGrants() {
  return readGrants().map(({ doc, site, kind, who, workbook, at }) => ({ doc, site, kind, who, workbook: workbook || "", at: at || "" }));
}

export function removeNetworkGrant(index) {
  const list = readGrants();
  if (index < 0 || index >= list.length) return false;
  list.splice(index, 1);
  writeGrants(list);
  return true;
}

// ---- Rate and concurrency ------------------------------------------------------

/// Document id → timestamps of its requests in the last minute.
const recent = new Map();
let active = 0;
const queue = [];

function underRate(doc) {
  const now = Date.now();
  const list = (recent.get(doc) || []).filter((t) => now - t < 60_000);
  if (list.length >= NET_LIMITS.perMinute) { recent.set(doc, list); return false; }
  list.push(now);
  recent.set(doc, list);
  return true;
}

function slot() {
  if (active < NET_LIMITS.concurrent) { active++; return Promise.resolve(); }
  return new Promise((r) => queue.push(r));
}

function release() {
  const next = queue.shift();
  if (next) next();
  else active--;
}

// ---- The request ---------------------------------------------------------------

class NetError extends Error {
  constructor(name, message) {
    super(message);
    this.name = name;
  }
}

/// Make the request with docs/99 §5a's fixed options and read at most 5 MB
/// of the reply as text. Resolves `{status, text}`; throws `NetError`.
async function perform(url, req) {
  const ctrl = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; ctrl.abort(); }, NET_LIMITS.timeoutMs);
  try {
    let res;
    try {
      res = await globalThis.fetch(url, {
        method: req.method,
        headers: req.headers,
        body: req.body,
        credentials: "omit",
        referrerPolicy: "no-referrer",
        cache: "no-store",
        mode: "cors",
        redirect: "error",
        signal: ctrl.signal,
      });
    } catch {
      if (timedOut) throw new NetError("NetworkError", `the site did not answer within ${NET_LIMITS.timeoutMs / 1000} s`);
      // The browser does not say which: the site refused this page (CORS), the
      // page's own policy refused the site (CSP), the site redirected, or the
      // network failed. None of them is worked around.
      throw new NetError("NetworkError", "the request failed (refused by the site or by this page's security policy, redirected, or the network is down)");
    }
    let text = "";
    if (res.body) {
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let total = 0;
      for (;;) {
        let chunk;
        try {
          chunk = await reader.read();
        } catch {
          if (timedOut) throw new NetError("NetworkError", `the site did not answer within ${NET_LIMITS.timeoutMs / 1000} s`);
          throw new NetError("NetworkError", "the reply was cut off");
        }
        if (chunk.done) break;
        total += chunk.value.byteLength;
        if (total > NET_LIMITS.maxResponseBytes) {
          // Stop reading and stop the transfer: nothing past the cap is kept.
          ctrl.abort();
          try { reader.cancel(); } catch {}
          throw new NetError("NetworkError", "the reply is larger than 5 MB");
        }
        text += decoder.decode(chunk.value, { stream: true });
      }
      text += decoder.decode();
    }
    return { status: res.status, text };
  } finally {
    clearTimeout(timer);
  }
}

// ---- The clock -------------------------------------------------------------------

/// The longest a call's or a run's clock is stopped for requests in flight, in
/// total. Waiting for the person to answer the Security Notice is not counted:
/// that is the person's time, not the script's.
export const NETWORK_HOLD_CAP_MS = 60_000;

/// A time limit that stops while the script waits on the page: a custom
/// function has 2 s and a request may take 10, so without this a function
/// that fetches would be cut off by its own request.
export class CallClock {
  constructor(onExpire) {
    this.onExpire = onExpire;
    this.timer = null;
    this.deadline = 0;
    this.remaining = 0;
    this.holds = 0;
    this.heldNet = 0;
    this.heldSince = 0;
    this.netHolds = 0;
    this.stopped = false;
  }
  arm(ms) {
    if (this.stopped) return;
    clearTimeout(this.timer);
    this.remaining = ms;
    if (this.holds === 0) this.run();
  }
  run() {
    this.deadline = Date.now() + this.remaining;
    this.timer = setTimeout(() => { if (!this.stopped) this.onExpire(); }, this.remaining);
  }
  /// `kind` is "consent" or "net".
  hold(kind) {
    if (this.stopped) return () => {};
    if (this.holds === 0 && this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
      this.remaining = Math.max(0, this.deadline - Date.now());
    }
    this.holds++;
    if (kind === "net" && this.netHolds++ === 0) this.heldSince = Date.now();
    let released = false;
    return () => {
      if (released || this.stopped) return;
      released = true;
      if (kind === "net" && --this.netHolds === 0) this.heldNet += Date.now() - this.heldSince;
      if (--this.holds === 0) {
        if (this.heldNet > NETWORK_HOLD_CAP_MS) this.remaining = 0;
        this.run();
      }
    };
  }
  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }
}

// ---- Serving a worker -----------------------------------------------------------

/// Answer a worker's network requests. `ctx`:
///
/// * `kind` — `"macro"` or `"functions"`;
/// * `sources` — `[{guid, name, source}]` this page loaded into the worker;
/// * `guid` — the macro's, for `"macro"`;
/// * `label(asker)` — what the Security Notice calls the asker, or `null`
///   when the asker is not one of `sources` (then nothing is asked);
/// * `clock()` — the `CallClock` running now, if any.
///
/// Returns a function that stops serving.
export function serveNetwork(worker, ctx) {
  let hashPromise = null;
  const hash = () => (hashPromise ??= sha256(JSON.stringify(
    ctx.sources.map((s) => [String(s.guid ?? ""), String(s.source ?? "")]).sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)),
  )));
  let live = true;
  const onMessage = async (ev) => {
    const m = ev.data;
    if (!m || m.type !== "net") return;
    const reply = (msg) => { if (live) { try { worker.postMessage({ type: "net-reply", id: m.id, ...msg }); } catch {} } };
    try {
      const res = await handle(m, ctx, hash);
      reply({ ok: true, status: res.status, text: res.text });
    } catch (err) {
      const name = err instanceof NetError ? err.name : "NetworkError";
      const message = err instanceof NetError ? err.message : "the request failed";
      if (live) status.textContent = `${ctx.kind === "macro" ? "macro" : "custom function"}: ${name}: ${message}`;
      reply({ ok: false, error: { name, message } });
    }
  };
  worker.addEventListener("message", onMessage);
  return () => {
    live = false;
    worker.removeEventListener("message", onMessage);
  };
}

async function handle(m, ctx, hash) {
  // From the moment the request reaches the page the script is waiting on
  // the page, not running: its clock stops now, not after the checks.
  let release1 = ctx.clock()?.hold("consent");
  try {
    const target = await admit(m, ctx, hash);
    const releaseNet = ctx.clock()?.hold("net");
    release1?.();
    release1 = null;
    try {
      await slot();
      try {
        return await perform(target.url, target.req);
      } finally {
        release();
      }
    } finally {
      releaseNet?.();
    }
  } finally {
    release1?.();
  }
}

/// Everything before the request is made: host policy, the address, the
/// request's shape, the rate, and the person's answer. Resolves the request
/// to make; throws `NetworkBlocked`.
async function admit(m, ctx, hash) {
  const blocked = (why) => new NetError("NetworkBlocked", why);
  const policy = networkPolicy();
  if (!policy.ok) throw blocked(policy.reason);
  const who = ctx.label(String(m.asker ?? ""));
  if (!who) throw blocked("the request did not come from this workbook's code");
  const target = checkUrl(m.request?.url, { pageOrigin: location.origin, collabOrigins: [...collabOrigins], allowlist: policy.allowlist });
  if (!target.ok) throw blocked(target.reason);
  const req = checkRequest(m.request);
  if (!req.ok) throw blocked(req.reason);
  const doc = await documentIdentity();
  const h = await hash();
  pruneGrants(doc, ctx.kind, ctx.guid, h);
  if (!underRate(doc)) throw blocked(`more than ${NET_LIMITS.perMinute} requests a minute from this workbook`);
  if (!hasGrant(doc, target.site, h)) {
    const allowed = await askConsent({ doc, site: target.site, hash: h, kind: ctx.kind, guid: ctx.guid, who });
    if (!allowed) throw blocked(`network access to ${target.site} was disabled`);
    // The answer took as long as the person did; the policy may have moved.
    const again = networkPolicy();
    if (!again.ok) throw blocked(again.reason);
  }
  return { url: target.url, req };
}

// ---- Consent: the Security Warning bar and the Security Notice -------------------

/// `grantKey` → `{ ask, waiters: [resolve] }` for prompts not yet answered.
const pending = new Map();
/// Prompts in the order they arrived; the Security Notice shows the first.
const order = [];
/// What the person disabled in this tab, until Enable Content.
const disabled = new Map();
let noticeOpen = null;

function askConsent(ask) {
  const key = grantKey(ask.doc, ask.site, ask.hash);
  if (disabled.has(key)) return Promise.resolve(false);
  return new Promise((resolve) => {
    const p = pending.get(key);
    // Asks for the same site, from the same sources, are one prompt.
    if (p) { p.waiters.push(resolve); return; }
    pending.set(key, { ask, waiters: [resolve] });
    order.push(key);
    renderBar();
    showNextNotice();
  });
}

function answer(key, allowed) {
  const p = pending.get(key);
  if (!p) return;
  pending.delete(key);
  const i = order.indexOf(key);
  if (i >= 0) order.splice(i, 1);
  if (allowed) {
    disabled.delete(key);
    addGrant({ doc: p.ask.doc, site: p.ask.site, hash: p.ask.hash, kind: p.ask.kind, guid: p.ask.guid || "", who: p.ask.who, workbook: documentName() || "" });
  } else {
    disabled.set(key, p.ask);
  }
  for (const w of p.waiters) w(allowed);
  renderBar();
}

/// Enable Content: everything this tab blocked or is asking about, as named on
/// the bar.
function enableAll() {
  for (const key of [...order]) answer(key, true);
  for (const [key, ask] of [...disabled]) {
    disabled.delete(key);
    addGrant({ doc: ask.doc, site: ask.site, hash: ask.hash, kind: ask.kind, guid: ask.guid || "", who: ask.who, workbook: documentName() || "" });
  }
  closeNotice();
  renderBar();
}

/// "The custom function PRICE", "The macro "Fetch rates"" — the asker, as the
/// Security Notice and the bar name it. Untrusted text: it only ever reaches
/// the page as a text node.
function askerPhrase(ask) {
  return ask.kind === "macro" ? `The macro "${ask.who}"` : `The custom function ${ask.who}`;
}

function barRoot() { return byId("oc-net-bar"); }

/// Excel's message bar: "SECURITY WARNING  External Data Connections have been
/// disabled  [Enable Content]", here naming what was blocked.
function renderBar() {
  const b = barRoot();
  if (!b) return;
  const asks = [...order.map((k) => pending.get(k)?.ask), ...disabled.values()].filter(Boolean);
  if (!asks.length) { b.hidden = true; b.replaceChildren(); return; }
  const row = el("div", "oc-recovery-row");
  const shield = el("span", "oc-net-bar-icon");
  // oc-safe-html: static markup, no interpolation.
  shield.innerHTML = '<svg viewBox="0 0 20 20" width="16" height="16" fill="currentColor" aria-hidden="true"><use href="#fi-shield"/></svg>';
  row.appendChild(shield);
  row.appendChild(el("strong", "oc-net-bar-title", "SECURITY WARNING"));
  const first = asks[0];
  const more = asks.length > 1 ? ` (and ${asks.length - 1} more)` : "";
  const detail = el("span", "oc-recovery-detail", ` Network access has been disabled: ${askerPhrase(first)} wants to connect to ${first.site}${more}.`);
  detail.id = "oc-net-bar-text";
  row.appendChild(detail);
  const actions = el("span", "oc-recovery-actions");
  const enable = el("button", "oc-btn", "Enable Content");
  enable.id = "oc-net-bar-enable";
  enable.addEventListener("click", enableAll);
  actions.appendChild(enable);
  const x = el("button", "oc-net-bar-x");
  x.id = "oc-net-bar-close";
  x.setAttribute("aria-label", "Close");
  x.title = "Close";
  // oc-safe-html: static markup, no interpolation.
  x.innerHTML = '<svg viewBox="0 0 20 20" width="14" height="14" fill="currentColor" aria-hidden="true"><use href="#fi-dismiss"/></svg>';
  // Closing the bar is not an answer, as in Excel: what is still being asked
  // is still asked in the Security Notice.
  x.addEventListener("click", () => { b.hidden = true; });
  actions.appendChild(x);
  row.appendChild(actions);
  b.replaceChildren(row);
  b.hidden = false;
}

function noticeRoot() { return byId("oc-net-notice"); }

function closeNotice() {
  const n = noticeRoot();
  if (!n) return;
  n.hidden = true;
  if (noticeOpen) {
    window.removeEventListener("keydown", noticeOpen.onKey, true);
    noticeOpen = null;
  }
  byId("oc-net-notice-body")?.replaceChildren();
}

/// Excel's *Microsoft Excel Security Notice* (data connections): "Microsoft
/// Office has identified a potential security concern." then what was
/// blocked and the risk, then Enable and Disable, in that order. Over any
/// dialog that is open, as Excel's is over its own windows.
function showNextNotice() {
  if (noticeOpen) return;
  const key = order[0];
  const p = key && pending.get(key);
  const n = noticeRoot();
  if (!p || !n) return;
  const { ask } = p;
  const body = byId("oc-net-notice-body");
  const lead = el("p", "oc-net-notice-lead", "OpenCalc has identified a potential security concern.");
  const what = el("p", "oc-confirm-text");
  what.id = "oc-net-notice-text";
  what.textContent = `Network access has been blocked. ${askerPhrase(ask)} wants to connect to ${ask.site}, and data from this workbook can be sent to it. If you choose to enable network access, data from this workbook may no longer be private. Do not enable this content unless you trust the source of this file.`;
  const row = el("div", "oc-confirm-actions");
  const enable = el("button", "oc-btn", "Enable");
  enable.id = "oc-net-notice-enable";
  const disable = el("button", "oc-btn", "Disable");
  disable.id = "oc-net-notice-disable";
  row.append(enable, disable);
  body.replaceChildren(lead, what, row);
  n.hidden = false;
  const decide = (allowed) => {
    closeNotice();
    answer(key, allowed);
    showNextNotice();
  };
  enable.addEventListener("click", () => decide(true));
  disable.addEventListener("click", () => decide(false));
  // Escape is Disable; Enter presses the focused button, which starts on
  // Disable — the safe answer is the one a stray key gives.
  const onKey = (e) => {
    if (n.hidden) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); decide(false); }
    else if (e.key === "Enter") {
      e.preventDefault(); e.stopPropagation();
      const a = document.activeElement;
      if (a === enable || (a && a === byId("oc-net-notice-enable"))) decide(true);
      else decide(false);
    } else if (e.key === "Tab") {
      e.preventDefault(); e.stopPropagation();
      (document.activeElement === enable ? disable : enable).focus();
    } else {
      // Modal: nothing behind it takes a key while it is up.
      e.stopPropagation();
    }
  };
  noticeOpen = { onKey };
  window.addEventListener("keydown", onKey, true);
  byId("oc-net-notice-x").onclick = () => decide(false);
  disable.focus();
}

// ---- File ▸ Options ▸ Trust Center -------------------------------------------------

/// Excel: File ▸ Options ▸ Trust Center ▸ Trust Center Settings… opens the
/// Trust Center, a list of categories with a page each. The one OpenCalc has
/// content for is External Content — Excel's page for data connections — and
/// it lists the network grants this device holds, each with Remove.
export async function openTrustCenter() {
  const { modalFrame } = await import("./editor.dialogs.js");
  const frame = modalFrame("Trust Center");
  const wrap = el("div", "oc-tc");
  const nav = el("div", "oc-tc-nav");
  nav.setAttribute("role", "tablist");
  const tab = el("button", "oc-tc-cat is-on", "External Content");
  tab.setAttribute("role", "tab");
  tab.setAttribute("aria-selected", "true");
  nav.appendChild(tab);
  const page = el("div", "oc-tc-page");
  page.id = "oc-tc-external";
  wrap.append(nav, page);
  const render = () => {
    page.replaceChildren();
    page.appendChild(el("h3", "oc-tc-heading", "Security settings for network access by macros and custom functions"));
    page.appendChild(el("p", "oc-tc-note", "Sites you allowed a workbook's code to connect to. Kept on this device only; never saved in the workbook or shared."));
    const grants = listNetworkGrants();
    if (!grants.length) {
      const none = el("p", "oc-tc-empty", "No sites are allowed.");
      none.id = "oc-tc-none";
      page.appendChild(none);
      return;
    }
    const list = el("div", "oc-tc-list");
    list.id = "oc-tc-grants";
    grants.forEach((g, i) => {
      const item = el("div", "oc-tc-item");
      item.dataset.site = g.site;
      const text = el("div", "oc-tc-item-text");
      text.appendChild(el("div", "oc-tc-site", g.site));
      const by = g.kind === "macro" ? `Macro "${g.who}"` : `Custom function ${g.who}`;
      text.appendChild(el("div", "oc-tc-who", `${by}${g.workbook ? ` in ${g.workbook}` : ""}`));
      const remove = el("button", "oc-btn", "Remove");
      remove.addEventListener("click", () => { removeNetworkGrant(i); render(); });
      item.append(text, remove);
      list.appendChild(item);
    });
    page.appendChild(list);
  };
  render();
  const row = el("div", "oc-confirm-actions");
  const ok = el("button", "oc-btn primary", "OK");
  ok.addEventListener("click", () => frame.close());
  row.appendChild(ok);
  frame.body.append(wrap, row);
  ok.focus();
}

// ---- Test hooks -----------------------------------------------------------------

/// Forget this tab's answers and the page-only grants, as a new tab would.
export function resetNetworkForTest() {
  for (const key of [...order]) answer(key, false);
  disabled.clear();
  recent.clear();
  memoryGrants = [];
  closeNotice();
  renderBar();
}
