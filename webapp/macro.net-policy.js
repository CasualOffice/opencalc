// Which network requests a macro or custom function may even ask for
// (`MACRO-13`, docs/99 §5a). Pure functions: no DOM, no engine, no `fetch` —
// the host's broker (`editor.network.js`) and its spec both import this.
//
// Everything here is decided on what `new URL` makes of the address, never on
// the text the script wrote: the WHATWG parser is what `fetch` itself uses, so
// it is the only reading of `https://0x7f.1/` (it is `127.0.0.1`) or
// `https://2130706433/` (also `127.0.0.1`) that matches the request the
// browser would make.

/// docs/99 §5a, in one place.
export const NET_LIMITS = Object.freeze({
  methods: Object.freeze(["GET", "HEAD", "POST"]),
  headers: Object.freeze(["accept", "content-type"]),
  maxRequestBytes: 1024 * 1024,
  maxResponseBytes: 5 * 1024 * 1024,
  timeoutMs: 10_000,
  concurrent: 4,
  perMinute: 60,
  /// Header values longer than this are refused (a header is not a body).
  maxHeaderValue: 1024,
  /// URLs longer than this are refused (Chrome's own limit is 2 MB; a URL is
  /// not a body either, and 1 MB of query string would be the body cap's way
  /// round).
  maxUrlLength: 8 * 1024,
});

/// Names that resolve only on a local network, or to this machine.
const LOCAL_SUFFIXES = [".local", ".localhost", ".internal", ".lan", ".home.arpa", ".test"];

/// `host` with a trailing dot removed and lower-cased: `Example.COM.` is the
/// same site as `example.com`, and a check that compared the two spellings
/// would let the first through a list that names the second.
export function normalHost(host) {
  return String(host ?? "").toLowerCase().replace(/\.+$/, "");
}

/// An IP literal as the URL parser leaves it: `[…]` for v6, dotted decimal for
/// v4 (every other v4 spelling — hex, octal, a single integer, fewer parts —
/// has already been rewritten to dotted decimal by `new URL`).
export function isIpLiteral(hostname) {
  const h = normalHost(hostname);
  if (h.startsWith("[") || h.includes(":")) return true;
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(h);
}

/// A host as a site entry on an allowlist: a bare host, or an origin. Anything
/// else is not a site and matches nothing.
function siteHost(entry) {
  const s = String(entry ?? "").trim();
  if (!s) return null;
  try {
    const u = new URL(s.includes("://") ? s : `https://${s}`);
    if (u.protocol !== "https:") return null;
    return normalHost(u.hostname);
  } catch {
    return null;
  }
}

function hostOf(origin) {
  if (!origin) return null;
  try { return normalHost(new URL(String(origin)).hostname); } catch { return null; }
}

/// Is `raw` an address a script may ask for?
///
/// `opts.pageOrigin` — OpenCalc's own origin; `opts.collabOrigins` — every
/// collaboration server this tab has used; `opts.allowlist` — the host's list
/// of sites, or `null` for none. Answers `{ ok: true, url, site }` where `site`
/// is `https://host` (what the consent prompt names), or `{ ok: false, reason }`.
export function checkUrl(raw, opts = {}) {
  const text = String(raw ?? "");
  if (text.length > NET_LIMITS.maxUrlLength) return { ok: false, reason: "the address is too long" };
  let u;
  try { u = new URL(text); } catch { return { ok: false, reason: "not an absolute address" }; }
  if (u.protocol !== "https:") return { ok: false, reason: "only https: addresses can be reached" };
  if (u.username || u.password) return { ok: false, reason: "an address may not carry a user name or password" };
  // `new URL` drops a port that is the scheme's default, so any port left is
  // another one.
  if (u.port !== "") return { ok: false, reason: "only the default port can be reached" };
  const host = normalHost(u.hostname);
  if (!host) return { ok: false, reason: "the address has no host" };
  if (isIpLiteral(host)) return { ok: false, reason: "IP addresses cannot be reached; use a site name" };
  if (!host.includes(".")) return { ok: false, reason: "single-label hosts cannot be reached" };
  if (host === "localhost" || LOCAL_SUFFIXES.some((s) => host.endsWith(s))) {
    return { ok: false, reason: "local-network names cannot be reached" };
  }
  const own = [opts.pageOrigin, ...(opts.collabOrigins || [])].map(hostOf).filter(Boolean);
  if (own.includes(host)) return { ok: false, reason: "OpenCalc's own servers cannot be reached from a workbook" };
  if (Array.isArray(opts.allowlist)) {
    const allowed = opts.allowlist.map(siteHost).filter(Boolean);
    if (!allowed.includes(host)) return { ok: false, reason: "this site is not on the host's list" };
  }
  const site = `https://${host}`;
  // The request goes to the normalised host, so what was checked is what is
  // fetched.
  u.hostname = host;
  return { ok: true, url: u.href, site };
}

/// The request a script asked for, cut down to what docs/99 §5a lets through:
/// `{ ok: true, method, headers, body }` or `{ ok: false, reason }`. `body` is
/// a string or `null`; `headers` a plain object of the two allowed names.
export function checkRequest(req) {
  const method = String(req?.method ?? "GET").toUpperCase();
  if (!NET_LIMITS.methods.includes(method)) return { ok: false, reason: `method ${method} is not allowed (GET, HEAD or POST)` };
  const headers = {};
  const given = req?.headers;
  if (given !== undefined && given !== null) {
    if (typeof given !== "object" || Array.isArray(given)) return { ok: false, reason: "headers must be an object" };
    for (const [k, v] of Object.entries(given)) {
      const name = String(k).toLowerCase();
      if (!NET_LIMITS.headers.includes(name)) return { ok: false, reason: `header ${k} is not allowed (only Accept and Content-Type)` };
      const value = String(v);
      if (value.length > NET_LIMITS.maxHeaderValue || /[\r\n\0]/.test(value)) return { ok: false, reason: `header ${k} is not a valid value` };
      headers[name] = value;
    }
  }
  let body = null;
  if (req?.body !== undefined && req?.body !== null) {
    if (typeof req.body !== "string") return { ok: false, reason: "a body must be a string (use JSON.stringify)" };
    if (method !== "POST") return { ok: false, reason: `a ${method} request cannot carry a body` };
    if (utf8Length(req.body) > NET_LIMITS.maxRequestBytes) return { ok: false, reason: "the body is larger than 1 MB" };
    body = req.body;
  }
  return { ok: true, method, headers, body };
}

/// Bytes `s` takes as UTF-8, without allocating them.
export function utf8Length(s) {
  let n = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x80) n += 1;
    else if (c < 0x800) n += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) { n += 4; i++; }
    else n += 3;
  }
  return n;
}
